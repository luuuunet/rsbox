// 自动故障恢复实现
use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct AutoRecovery {
    max_recovery_attempts: usize,
    recovery_strategies: Vec<RecoveryStrategy>,
    last_recovery: Arc<RwLock<Option<Instant>>>,
    cooldown: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryStrategy {
    Restart,
    SwitchServer,
    ResetConnection,
    ClearCache,
    WaitAndRetry,
}

#[derive(Debug, Clone)]
pub enum FailureType {
    ConnectionTimeout,
    ConnectionRefused,
    NetworkUnreachable,
    TooManyConnections,
    AuthenticationFailed,
    Unknown(String),
}

impl AutoRecovery {
    pub fn new() -> Self {
        Self {
            max_recovery_attempts: 3,
            recovery_strategies: vec![
                RecoveryStrategy::WaitAndRetry,
                RecoveryStrategy::ResetConnection,
                RecoveryStrategy::SwitchServer,
                RecoveryStrategy::ClearCache,
                RecoveryStrategy::Restart,
            ],
            last_recovery: Arc::new(RwLock::new(None)),
            cooldown: Duration::from_secs(60),
        }
    }

    /// 处理故障
    pub async fn handle_failure(&self, failure: FailureType) -> Result<RecoveryAction> {
        // 检查冷却时间
        if !self.can_recover().await {
            tracing::warn!("Recovery in cooldown, skipping");
            return Ok(RecoveryAction::Skip);
        }

        // 选择恢复策略
        let strategy = self.select_strategy(&failure);

        tracing::info!(
            failure = ?failure,
            strategy = ?strategy,
            "Attempting recovery"
        );

        // 执行恢复
        let result = self.execute_recovery(&strategy).await;

        // 更新最后恢复时间
        let mut last = self.last_recovery.write().await;
        *last = Some(Instant::now());

        result
    }

    /// 选择恢复策略
    fn select_strategy(&self, failure: &FailureType) -> RecoveryStrategy {
        match failure {
            FailureType::ConnectionTimeout => RecoveryStrategy::WaitAndRetry,
            FailureType::ConnectionRefused => RecoveryStrategy::SwitchServer,
            FailureType::NetworkUnreachable => RecoveryStrategy::WaitAndRetry,
            FailureType::TooManyConnections => RecoveryStrategy::ResetConnection,
            FailureType::AuthenticationFailed => RecoveryStrategy::Restart,
            FailureType::Unknown(_) => RecoveryStrategy::WaitAndRetry,
        }
    }

    /// 执行恢复
    async fn execute_recovery(&self, strategy: &RecoveryStrategy) -> Result<RecoveryAction> {
        match strategy {
            RecoveryStrategy::WaitAndRetry => {
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(RecoveryAction::Retry)
            }
            RecoveryStrategy::ResetConnection => {
                // 重置连接
                Ok(RecoveryAction::Reset)
            }
            RecoveryStrategy::SwitchServer => {
                // 切换服务器
                Ok(RecoveryAction::Switch)
            }
            RecoveryStrategy::ClearCache => {
                // 清理缓存
                Ok(RecoveryAction::ClearCache)
            }
            RecoveryStrategy::Restart => {
                // 重启（需要上层处理）
                Ok(RecoveryAction::Restart)
            }
        }
    }

    /// 检查是否可以恢复
    async fn can_recover(&self) -> bool {
        let last = self.last_recovery.read().await;

        match *last {
            Some(instant) => instant.elapsed() > self.cooldown,
            None => true,
        }
    }

    /// 多次尝试恢复
    pub async fn recover_with_retries(
        &self,
        failure: FailureType,
    ) -> Result<RecoveryAction> {
        for attempt in 1..=self.max_recovery_attempts {
            tracing::debug!(
                attempt = attempt,
                max = self.max_recovery_attempts,
                "Recovery attempt"
            );

            match self.handle_failure(failure.clone()).await {
                Ok(action) if action != RecoveryAction::Skip => {
                    tracing::info!(
                        attempt = attempt,
                        action = ?action,
                        "Recovery successful"
                    );
                    return Ok(action);
                }
                Ok(_) => {
                    tracing::debug!("Recovery skipped, waiting");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
                Err(e) => {
                    tracing::warn!(
                        attempt = attempt,
                        error = %e,
                        "Recovery attempt failed"
                    );

                    if attempt < self.max_recovery_attempts {
                        tokio::time::sleep(Duration::from_secs(5 * attempt as u64)).await;
                    }
                }
            }
        }

        anyhow::bail!("All recovery attempts failed")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryAction {
    Retry,
    Reset,
    Switch,
    ClearCache,
    Restart,
    Skip,
}

impl Default for AutoRecovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_auto_recovery() {
        let recovery = AutoRecovery::new();

        let action = recovery
            .handle_failure(FailureType::ConnectionTimeout)
            .await
            .unwrap();

        assert_eq!(action, RecoveryAction::Retry);
    }

    #[tokio::test]
    async fn test_recovery_cooldown() {
        let recovery = AutoRecovery {
            cooldown: Duration::from_millis(100),
            ..AutoRecovery::new()
        };

        // 第一次应该成功
        let result1 = recovery
            .handle_failure(FailureType::ConnectionTimeout)
            .await;
        assert!(result1.is_ok());

        // 立即第二次应该被跳过
        let result2 = recovery
            .handle_failure(FailureType::ConnectionTimeout)
            .await
            .unwrap();
        assert_eq!(result2, RecoveryAction::Skip);

        // 等待冷却后应该成功
        tokio::time::sleep(Duration::from_millis(150)).await;
        let result3 = recovery
            .handle_failure(FailureType::ConnectionTimeout)
            .await;
        assert!(result3.is_ok());
    }
}
