//! External sidecar helper for legacy REALITY outbounds (not used by RSQ/RST/Hy2).

use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

static SIDECAR: OnceLock<Mutex<Option<SidecarState>>> = OnceLock::new();

struct SidecarState {
    port: u16,
    _child: Child,
    config_path: PathBuf,
}

fn sidecar_slot() -> &'static Mutex<Option<SidecarState>> {
    SIDECAR.get_or_init(|| Mutex::new(None))
}

/// Resolve an optional external sidecar binary (`RSBOX_SIDECAR_PATH`).
pub fn find_sidecar_binary() -> Option<PathBuf> {
    for key in ["RSBOX_SIDECAR_PATH"] {
        if let Ok(p) = std::env::var(key) {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["rsbox-sidecar.exe", "rsbox-sidecar"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    which_sidecar()
}

fn which_sidecar() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        for dir in std::env::split_paths(&paths) {
            for name in ["rsbox-sidecar.exe", "rsbox-sidecar"] {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    })
}

fn pick_port() -> u16 {
    18000 + (std::process::id() as u16 % 4000)
}

fn sidecar_config(vless: &Value, listen_port: u16) -> Value {
    let mut outbound = serde_json::Map::new();
    outbound.insert("type".into(), json!("vless"));
    outbound.insert("tag".into(), json!("proxy"));
    for key in [
        "server",
        "server_port",
        "uuid",
        "flow",
        "network",
        "packet_encoding",
        "tls",
    ] {
        if let Some(v) = vless.get(key) {
            if !v.is_null() {
                outbound.insert(key.into(), v.clone());
            }
        }
    }
    json!({
        "log": { "level": "warn" },
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": listen_port
        }],
        "outbounds": [
            Value::Object(outbound),
            json!({ "type": "direct", "tag": "direct" })
        ],
        "route": { "final": "proxy" }
    })
}

/// Start an external sidecar for a legacy VLESS+REALITY outbound if not already running.
pub fn ensure(vless: &Value) -> Result<u16> {
    let mut slot = sidecar_slot().lock().expect("sidecar lock");
    if let Some(state) = slot.as_ref() {
        return Ok(state.port);
    }
    let sidecar = find_sidecar_binary().context(
        "REALITY legacy path needs RSBOX_SIDECAR_PATH pointing to an external proxy binary",
    )?;
    let port = pick_port();
    let dir = std::env::temp_dir().join(format!("rsbox-reality-{}", std::process::id()));
    std::fs::create_dir_all(&dir).context("create sidecar temp dir")?;
    let config_path = dir.join("sidecar.json");
    let config = sidecar_config(vless, port);
    std::fs::write(&config_path, serde_json::to_vec_pretty(&config)?).context("write sidecar config")?;

    let check = Command::new(&sidecar)
        .args(["check", "-c"])
        .arg(&config_path)
        .output()
        .context("sidecar check")?;
    if !check.status.success() {
        anyhow::bail!(
            "sidecar check failed: {}",
            String::from_utf8_lossy(&check.stderr)
        );
    }

    let child = Command::new(&sidecar)
        .args(["run", "-c"])
        .arg(&config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn {}", sidecar.display()))?;

    std::thread::sleep(Duration::from_millis(800));
    *slot = Some(SidecarState {
        port,
        _child: child,
        config_path,
    });
    Ok(port)
}

pub struct SidecarOutbound {
    tag: String,
    port: u16,
}

impl SidecarOutbound {
    pub fn new(tag: String, port: u16) -> Self {
        Self { tag, port }
    }

    async fn connect_via_mixed(&self, destination: SocketAddr, domain: Option<&str>) -> Result<TcpStream> {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", self.port))
            .await
            .context("connect legacy sidecar")?;
        let target = if let Some(name) = domain.filter(|d| !d.is_empty()) {
            format!("{name}:{}", destination.port())
        } else {
            match destination {
                SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
                SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
            }
        };
        let req = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n");
        stream.write_all(req.as_bytes()).await?;
        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await?;
        let resp = std::str::from_utf8(&buf[..n]).unwrap_or("");
        if !resp.contains("200") {
            anyhow::bail!("sidecar CONNECT failed: {resp}");
        }
        Ok(stream)
    }
}

#[async_trait]
impl Outbound for SidecarOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        rsb_constant::TYPE_VLESS
    }

    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }

    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        Ok(Box::new(
            self.connect_via_mixed(destination, domain)
                .await
                .map_err(Into::<BoxError>::into)?,
        ))
    }

    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let stream = self
            .connect_via_mixed(destination, None)
            .await
            .map_err(Into::<BoxError>::into)?;
        Ok(crate::udp_over_tcp::tunneled_udp(stream).await)
    }

    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}
