// 配置热重载实现
use anyhow::{Context, Result};
use rsb_config::Options;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct RuntimeReloader {
    runtime: Arc<RwLock<Runtime>>,
}

impl RuntimeReloader {
    pub fn new(runtime: Arc<RwLock<Runtime>>) -> Self {
        Self { runtime }
    }

    pub async fn reload(&self, new_config: Options) -> Result<()> {
        tracing::info!("Starting configuration reload");

        // 验证新配置
        self.validate_config(&new_config)?;

        // 获取运行时写锁
        let mut runtime = self.runtime.write().await;

        tracing::info!("Stopping old services");

        // 停止旧的入站
        for inbound in &runtime.inbounds {
            if let Err(e) = inbound.close().await {
                tracing::warn!("Failed to close inbound {}: {}", inbound.tag(), e);
            }
        }

        // 停止旧的出站（部分）
        // 注意：保持活跃连接

        tracing::info!("Starting new services");

        // 应用新配置
        runtime.options = new_config.clone();

        // 重新构建路由
        runtime.rebuild_router()?;

        // 启动新的入站
        runtime.start_inbounds().await?;

        // 更新出站
        runtime.update_outbounds()?;

        tracing::info!("Configuration reload completed successfully");

        Ok(())
    }

    fn validate_config(&self, config: &Options) -> Result<()> {
        // 验证入站配置
        if config.inbounds.is_empty() {
            anyhow::bail!("At least one inbound is required");
        }

        // 验证出站配置
        if config.outbounds.is_empty() {
            anyhow::bail!("At least one outbound is required");
        }

        // 验证路由规则
        if let Some(route) = &config.route {
            for rule in &route.rules {
                // 验证规则的出站是否存在
                if let Some(outbound) = &rule.outbound {
                    if !config.outbounds.iter().any(|ob| &ob.tag == outbound) {
                        anyhow::bail!("Rule references non-existent outbound: {}", outbound);
                    }
                }
            }
        }

        Ok(())
    }

    /// 从文件重新加载配置
    pub async fn reload_from_file(&self, path: &str) -> Result<()> {
        tracing::info!(path = %path, "Reloading configuration from file");

        let content = tokio::fs::read_to_string(path)
            .await
            .context("Failed to read config file")?;

        let new_config: Options = serde_json::from_str(&content)
            .context("Failed to parse config file")?;

        self.reload(new_config).await
    }

    /// 监听配置文件变化并自动重载
    pub async fn watch_config_file(&self, path: String) -> Result<()> {
        use notify::{Watcher, RecursiveMode, watcher};
        use std::sync::mpsc::channel;
        use std::time::Duration;

        let (tx, rx) = channel();

        let mut watcher = watcher(tx, Duration::from_secs(2))?;
        watcher.watch(&path, RecursiveMode::NonRecursive)?;

        tracing::info!(path = %path, "Watching configuration file for changes");

        loop {
            match rx.recv() {
                Ok(event) => {
                    use notify::DebouncedEvent;
                    match event {
                        DebouncedEvent::Write(_) | DebouncedEvent::Create(_) => {
                            tracing::info!("Configuration file changed, reloading");
                            if let Err(e) = self.reload_from_file(&path).await {
                                tracing::error!("Failed to reload configuration: {}", e);
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    tracing::error!("Watch error: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }
}

// Runtime 扩展方法
use crate::Runtime;

impl Runtime {
    fn rebuild_router(&mut self) -> Result<()> {
        // 重新构建路由器
        tracing::debug!("Rebuilding router");
        // TODO: 实现路由器重建逻辑
        Ok(())
    }

    async fn start_inbounds(&mut self) -> Result<()> {
        // 启动所有入站
        tracing::debug!("Starting inbounds");
        // TODO: 实现入站启动逻辑
        Ok(())
    }

    fn update_outbounds(&mut self) -> Result<()> {
        // 更新出站配置
        tracing::debug!("Updating outbounds");
        // TODO: 实现出站更新逻辑
        Ok(())
    }
}
