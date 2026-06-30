use anyhow::Context;
use clap::{Parser, Subcommand};
use rsb_api::{CacheFileService, ClashApiServer, V2RayApiServer};
use rsb_config::Options;
use rsb_constant::VERSION;
use rsb_protocol::RsBox;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { config } => run(config).await,
        Commands::Check { config } => check(&config),
        Commands::Version => {
            println!("rsbox {VERSION} (sing-box compatible, Rust)");
            Ok(())
        },
    }
}

fn check(path: &str) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(path).context("read config")?;
    let options = Options::from_json(&text)?;
    let mut warnings = Vec::new();
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

async fn run(path: String) -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let text = std::fs::read_to_string(&path).context("read config")?;
    let options = Options::from_json(&text)?;
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
