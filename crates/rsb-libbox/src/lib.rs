//! C ABI for embedding rsbox (libbox-compatible subset).

use rsb_api::{CacheFileService, ClashApiServer, V2RayApiServer};
use rsb_constant::VERSION;
use rsb_protocol::RsBox;
use std::ffi::{c_char, CStr};
use std::sync::Mutex;
use tokio::runtime::Runtime;

struct LibBoxState {
    runtime: Runtime,
    instance: Option<RsBox>,
    clash: ClashApiServer,
    v2ray: Option<V2RayApiServer>,
    cache: Option<CacheFileService>,
    _quic_block: Option<rsb_core::QuicBlockGuard>,
}

static STATE: Mutex<Option<LibBoxState>> = Mutex::new(None);

fn cstr_to_string(ptr: *const c_char) -> anyhow::Result<String> {
    if ptr.is_null() {
        anyhow::bail!("null pointer");
    }
    Ok(unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned())
}

async fn start_from_options(
    options: rsb_config::Options,
) -> anyhow::Result<(
    RsBox,
    ClashApiServer,
    Option<V2RayApiServer>,
    Option<CacheFileService>,
    Option<rsb_core::QuicBlockGuard>,
)> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&options.log.level))
        .try_init()
        .ok();
    let mut cache = None;
    if let Some(exp) = &options.experimental {
        if let Some(cfg) = &exp.cache_file {
            cache = Some(CacheFileService::start(cfg).await?);
        }
    }
    let quic_block = if options
        .experimental
        .as_ref()
        .map(|e| e.block_quic)
        .unwrap_or(false)
    {
        let allow = options.udp_tunnel_endpoints();
        rsb_core::QuicBlockGuard::try_install(&allow)
    } else {
        None
    };
    let instance = RsBox::new(options.clone()).await?;
    if let Some(cache_svc) = cache.as_ref() {
        instance
            .controller()
            .restore_selectors(&cache_svc.selectors());
    }
    let cache_arc = cache.clone();
    let mut clash = ClashApiServer::new();
    let mut v2ray = None;
    if let Some(exp) = &options.experimental {
        if let Some(api) = &exp.clash_api {
            clash
                .start(
                    api,
                    instance.controller(),
                    instance.connections(),
                    cache_arc.map(std::sync::Arc::new),
                )
                .await?;
        }
        if let Some(cfg) = &exp.v2ray_api {
            v2ray = Some(V2RayApiServer::start(cfg, instance.connections()).await?);
        }
    }
    instance.start().await?;
    Ok((instance, clash, v2ray, cache, quic_block))
}

fn start_with_options(options: rsb_config::Options) -> i32 {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return -2,
    };
    if guard.is_some() {
        return -3;
    }
    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return -4,
    };
    match runtime.block_on(start_from_options(options)) {
        Ok((instance, clash, v2ray, cache, quic_block)) => {
            *guard = Some(LibBoxState {
                runtime,
                instance: Some(instance),
                clash,
                v2ray,
                cache,
                _quic_block: quic_block,
            });
            0
        },
        Err(err) => {
            tracing::error!(error = %err, "rsbox start failed");
            -5
        },
    }
}

/// Returns rsbox version string (static, do not free).
#[no_mangle]
pub extern "C" fn rsbox_version() -> *const c_char {
    static VER: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VER.get_or_init(|| format!("{VERSION}\0")).as_ptr() as *const c_char
}

/// Parse config JSON; returns 0 on success.
#[no_mangle]
pub extern "C" fn rsbox_check_config(config_json: *const c_char) -> i32 {
    match cstr_to_string(config_json)
        .and_then(|text| rsb_config::Options::from_json(&text).map(|_| ()))
    {
        Ok(()) => 0,
        Err(err) => {
            tracing::error!(error = %err, "rsbox_check_config failed");
            -1
        },
    }
}

/// Start rsbox from config JSON path. Returns 0 on success.
#[no_mangle]
pub extern "C" fn rsbox_start(config_path: *const c_char) -> i32 {
    let path = match cstr_to_string(config_path) {
        Ok(p) => p,
        Err(_) => return -1,
    };
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return -2,
    };
    if guard.is_some() {
        return -3;
    }
    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return -4,
    };
    let result = runtime.block_on(async {
        let text = tokio::fs::read_to_string(&path).await?;
        let options = rsb_config::Options::from_json(&text)?;
        start_from_options(options).await
    });
    match result {
        Ok((instance, clash, v2ray, cache, quic_block)) => {
            *guard = Some(LibBoxState {
                runtime,
                instance: Some(instance),
                clash,
                v2ray,
                cache,
                _quic_block: quic_block,
            });
            0
        },
        Err(err) => {
            tracing::error!(error = %err, "rsbox_start failed");
            -5
        },
    }
}

/// Start rsbox from in-memory config JSON (preferred on Android/iOS).
#[no_mangle]
pub extern "C" fn rsbox_start_config(config_json: *const c_char) -> i32 {
    let options =
        match cstr_to_string(config_json).and_then(|text| rsb_config::Options::from_json(&text)) {
            Ok(opts) => opts,
            Err(_) => return -1,
        };
    start_with_options(options)
}

/// Stop rsbox if running.
#[no_mangle]
pub extern "C" fn rsbox_stop() -> i32 {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return -1,
    };
    let Some(state) = guard.take() else {
        return 0;
    };
    let LibBoxState {
        runtime,
        instance,
        mut clash,
        v2ray,
        cache,
        _quic_block,
    } = state;
    runtime.block_on(async {
        if let Some(instance) = instance {
            instance.close().await.ok();
        }
        clash.stop();
        if let Some(mut v) = v2ray {
            v.stop();
        }
        if let Some(c) = cache {
            c.flush().await.ok();
        }
    });
    0
}

/// Human-readable version for mobile shells.
#[no_mangle]
pub extern "C" fn rsbox_version_full() -> *const c_char {
    static FULL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    FULL.get_or_init(|| format!("rsbox {VERSION} (libbox FFI)\0"))
        .as_ptr() as *const c_char
}
