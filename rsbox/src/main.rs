use anyhow::Context;
use clap::{Parser, Subcommand};
use rsb_api::{CacheFileService, ClashApiServer, V2RayApiServer};
use rsb_config::Options;
use rsb_constant::VERSION;
use rsb_protocol::RsBox;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "rsbox", about = "Rust sing-box compatible proxy platform")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long)]
        config: String,
    },
    Check {
        #[arg(short, long)]
        config: String,
    },
    Version,
    /// Generate self-signed TLS cert/key for local RSQ (no Let's Encrypt needed).
    RsqGenCert {
        #[arg(long, default_value = "examples/certs/rsq-local")]
        output_dir: String,
        #[arg(long, default_value = "rsq.local")]
        name: String,
    },
    /// Print an `rsq://` share link for clients / subscriptions.
    RsqGenLink {
        #[arg(long)]
        server: String,
        #[arg(long, default_value_t = 18443)]
        port: u16,
        #[arg(long)]
        password: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        sni: Option<String>,
        #[arg(long)]
        up_mbps: Option<u32>,
        #[arg(long)]
        down_mbps: Option<u32>,
        #[arg(long, default_value = "video")]
        profile: String,
        #[arg(long)]
        obfs_password: Option<String>,
        #[arg(long)]
        obfs_version: Option<u8>,
        #[arg(long, default_value_t = false)]
        insecure: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { config } => run(config).await,
        Commands::Check { config } => check(&config).await,
        Commands::Version => {
            println!("rsbox {VERSION} (sing-box compatible, Rust)");
            Ok(())
        },
        Commands::RsqGenCert { output_dir, name } => {
            let dir = std::path::Path::new(&output_dir);
            let (cert, key) = rsb_protocol::rsq::write_dev_certs(dir, &name)?;
            println!("Wrote RSQ dev TLS files:");
            println!("  {}", cert.display());
            println!("  {}", key.display());
            println!("Use with examples/rsq-local-server.json (client: tls.insecure=true)");
            Ok(())
        },
        Commands::RsqGenLink {
            server,
            port,
            password,
            name,
            sni,
            up_mbps,
            down_mbps,
            profile,
            obfs_password,
            obfs_version,
            insecure,
        } => {
            let link = rsb_protocol::rsq::RsqShareLink {
                password: &password,
                server: &server,
                port,
                name: name.as_deref(),
                sni: sni.as_deref().or(Some(server.as_str())),
                up_mbps,
                down_mbps,
                profile: Some(profile.as_str()),
                obfs_password: obfs_password.as_deref(),
                obfs_version,
                insecure,
            }
            .encode();
            println!("{link}");
            Ok(())
        },
    }
}

async fn check(path: &str) -> anyhow::Result<()> {
    let (config_path, config_dir) = load_config_paths(path)?;
    let text = std::fs::read_to_string(&config_path).context("read config")?;
    let mut options = Options::from_json(&text)?;
    let sub_count = rsb_core::merge_outbound_providers(&mut options, config_dir.as_deref()).await?;
    if sub_count > 0 {
        println!("Subscription: loaded {sub_count} outbound(s)");
    }
    options.validate().context("validate config after subscription merge")?;
    let mut warnings = collect_config_warnings(&options, config_dir.as_deref(), sub_count);
    for (i, ib) in options.inbounds.iter().enumerate() {
        if !rsb_protocol::is_known_inbound(&ib.kind) {
            warnings.push(format!("inbound[{i}] unknown type: {}", ib.kind));
        }
    }
    for (i, ob) in options.outbounds.iter().enumerate() {
        if !rsb_protocol::is_known_outbound(&ob.kind) {
            warnings.push(format!("outbound[{i}] unknown type: {}", ob.kind));
        }
    }
    for (i, ep) in options.endpoints.iter().enumerate() {
        if !rsb_protocol::is_known_endpoint(&ep.kind) {
            warnings.push(format!("endpoint[{i}] unknown type: {}", ep.kind));
        }
    }
    for (i, svc) in options.services.iter().enumerate() {
        if !rsb_protocol::is_known_service(&svc.kind) {
            warnings.push(format!(
                "service[{i}] unknown type: {} (will use generic stub)",
                svc.kind
            ));
        }
    }
    println!(
        "OK: inbounds={}, outbounds={}, route.final={:?}",
        options.inbounds.len(),
        options.outbounds.len(),
        options.route_final()
    );
    if !warnings.is_empty() {
        println!("Warnings:");
        for w in &warnings {
            println!("  - {w}");
        }
    }
    Ok(())
}

fn load_config_paths(path: &str) -> anyhow::Result<(PathBuf, Option<PathBuf>)> {
    let config_path = std::path::Path::new(path);
    let canonical = config_path
        .canonicalize()
        .with_context(|| format!("resolve config path `{path}`"))?;
    let config_dir = canonical.parent().map(Path::to_path_buf);
    Ok((canonical, config_dir))
}

fn apply_config_working_dir(config_dir: Option<&Path>) -> anyhow::Result<()> {
    let Some(dir) = config_dir else {
        return Ok(());
    };
    std::env::set_current_dir(dir).with_context(|| format!("set cwd to `{}`", dir.display()))?;
    Ok(())
}

fn resolve_config_relative(path: &str, config_dir: Option<&Path>) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        return candidate;
    }
    let mut paths = Vec::new();
    if let Some(dir) = config_dir {
        paths.push(dir.join(&candidate));
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(&candidate));
    }
    for p in &paths {
        if p.exists() {
            return p.clone();
        }
    }
    paths.into_iter().next().unwrap_or(candidate)
}

fn collect_config_warnings(
    options: &Options,
    config_dir: Option<&Path>,
    sub_count: usize,
) -> Vec<String> {
    let mut warnings = Vec::new();
    if !options.outbound_providers.is_empty() && sub_count == 0 {
        warnings.push(
            "subscription provider(s) configured but 0 outbounds loaded".to_string(),
        );
    }
    if let Some(final_tag) = options.route_final() {
        let tags: HashSet<String> = options
            .outbounds
            .iter()
            .filter_map(|ob| ob.tag.clone())
            .collect();
        if !tags.contains(final_tag) {
            warnings.push(format!(
                "route.final references unknown outbound tag: {final_tag}"
            ));
        }
    }
    for (i, ib) in options.inbounds.iter().enumerate() {
        if ib.kind != "rsq" {
            continue;
        }
        let Some(tls) = ib.raw.get("tls") else {
            continue;
        };
        for key in ["certificate_path", "key_path"] {
            let Some(raw) = tls.get(key).and_then(|v| v.as_str()) else {
                continue;
            };
            let resolved = resolve_config_relative(raw, config_dir);
            if !resolved.exists() {
                warnings.push(format!(
                    "inbound[{i}] rsq tls.{key} not found: {}",
                    resolved.display()
                ));
            }
        }
    }
    warnings
}

async fn run(path: String) -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let (config_path, config_dir) = load_config_paths(&path)?;
    apply_config_working_dir(config_dir.as_deref())?;
    let text = std::fs::read_to_string(&config_path).context("read config")?;
    let mut options = Options::from_json(&text)?;
    let sub_count = rsb_core::merge_outbound_providers(&mut options, config_dir.as_deref()).await?;
    if sub_count > 0 {
        tracing::info!(count = sub_count, "loaded outbounds from subscription providers");
    }
    options.validate().context("validate config after subscription merge")?;
    init_tracing(&options.log.level);

    let mut cache = None;
    if let Some(exp) = &options.experimental {
        if let Some(cache_cfg) = &exp.cache_file {
            cache = Some(CacheFileService::start(cache_cfg).await?);
        }
    }

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
        if let Some(v2ray_cfg) = &exp.v2ray_api {
            v2ray = Some(V2RayApiServer::start(v2ray_cfg, instance.connections()).await?);
        }
    }

    instance.start().await?;
    tracing::info!("rsbox running — Ctrl+C to stop");
    tokio::signal::ctrl_c().await?;
    instance.close().await?;
    clash.stop();
    if let Some(mut v) = v2ray {
        v.stop();
    }
    if let Some(c) = cache {
        c.flush().await.ok();
    }
    Ok(())
}

fn init_tracing(level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
