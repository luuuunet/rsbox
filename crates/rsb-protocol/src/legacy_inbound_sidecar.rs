//! External sidecar for legacy inbounds rsbox does not implement natively.

use crate::reality_sidecar;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

pub struct LegacyInboundSidecar {
    tag: String,
    kind: String,
    inbound: Value,
    shutdown: tokio::sync::watch::Sender<bool>,
    child: Mutex<Option<Child>>,
    config_path: Mutex<Option<PathBuf>>,
}

impl LegacyInboundSidecar {
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
impl Inbound for LegacyInboundSidecar {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        &self.kind
    }

    async fn start(&self) -> Result<(), BoxError> {
        let sidecar = reality_sidecar::find_sidecar_binary().context(
            "legacy inbound sidecar: set RSBOX_SIDECAR_PATH to an external proxy binary",
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

        let check = Command::new(&sidecar)
            .args(["check", "-c"])
            .arg(&config_path)
            .output()
            .context("sidecar check inbound")?;
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
        *self.child.lock().expect("sidecar lock") = Some(child);
        *self.config_path.lock().expect("sidecar lock") = Some(config_path);
        tracing::info!(
            tag = %self.tag,
            kind = %self.kind,
            "legacy inbound sidecar started"
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
