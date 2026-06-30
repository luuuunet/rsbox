//! sing-box sidecar for inbounds rsbox does not implement natively (e.g. VLESS+REALITY).

use crate::reality_sidecar;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

pub struct SingboxInboundSidecar {
    tag: String,
    kind: String,
    inbound: Value,
    shutdown: tokio::sync::watch::Sender<bool>,
    child: Mutex<Option<Child>>,
    config_path: Mutex<Option<PathBuf>>,
}

impl SingboxInboundSidecar {
    pub fn new(tag: String, kind: String, inbound: Value) -> Self {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Self {
            tag,
            kind,
            inbound,
            shutdown,
            child: Mutex::new(None),
            config_path: Mutex::new(None),
        }
    }

    fn sidecar_config(inbound: &Value) -> Value {
        json!({
            "log": { "level": "warn" },
            "inbounds": [inbound.clone()],
            "outbounds": [{ "type": "direct", "tag": "direct" }],
            "route": { "final": "direct" }
        })
    }
}

#[async_trait]
impl Inbound for SingboxInboundSidecar {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        &self.kind
    }

    async fn start(&self) -> Result<(), BoxError> {
        let singbox = reality_sidecar::find_singbox().context(
            "sing-box sidecar inbound: set RSBOX_SINGBOX_PATH or place sing-box next to rsbox",
        )?;
        let dir = std::env::temp_dir().join(format!(
            "rsbox-inbound-{}-{}",
            self.tag.replace('/', "_"),
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).context("create inbound sidecar dir")?;
        let config_path = dir.join("inbound-sidecar.json");
        let config = Self::sidecar_config(&self.inbound);
        std::fs::write(&config_path, serde_json::to_vec_pretty(&config)?)
            .context("write inbound sidecar config")?;

        let check = Command::new(&singbox)
            .args(["check", "-c"])
            .arg(&config_path)
            .output()
            .context("sing-box check inbound sidecar")?;
        if !check.status.success() {
            anyhow::bail!(
                "sing-box check failed: {}",
                String::from_utf8_lossy(&check.stderr)
            );
        }

        let child = Command::new(&singbox)
            .args(["run", "-c"])
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {}", singbox.display()))?;

        std::thread::sleep(Duration::from_millis(800));
        *self.child.lock().expect("sidecar lock") = Some(child);
        *self.config_path.lock().expect("sidecar lock") = Some(config_path);
        tracing::info!(
            tag = %self.tag,
            kind = %self.kind,
            "sing-box inbound sidecar started"
        );
        Ok(())
    }

    async fn close(&self) -> Result<(), BoxError> {
        let _ = self.shutdown.send(true);
        if let Some(mut child) = self.child.lock().expect("sidecar lock").take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}
