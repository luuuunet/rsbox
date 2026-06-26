// 会话持久化实现
use anyhow::Result;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs;
use tokio::time::interval;

pub struct SessionManager {
    sessions: Arc<DashMap<String, Session>>,
    storage_path: PathBuf,
    auto_save_interval: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub remote_addr: String,
    pub created_at: SystemTime,
    pub last_active: SystemTime,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub metadata: serde_json::Value,
}

impl SessionManager {
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            storage_path,
            auto_save_interval: Duration::from_secs(300), // 5 分钟
        }
    }

    /// 创建新会话
    pub fn create_session(&self, id: String, remote_addr: String) -> Session {
        let session = Session {
            id: id.clone(),
            remote_addr,
            created_at: SystemTime::now(),
            last_active: SystemTime::now(),
            bytes_sent: 0,
            bytes_received: 0,
            metadata: serde_json::json!({}),
        };

        self.sessions.insert(id, session.clone());
        tracing::debug!(session_id = %session.id, "Session created");

        session
    }

    /// 更新会话
    pub fn update_session<F>(&self, id: &str, update_fn: F)
    where
        F: FnOnce(&mut Session),
    {
        if let Some(mut session) = self.sessions.get_mut(id) {
            update_fn(&mut session);
            session.last_active = SystemTime::now();
        }
    }

    /// 删除会话
    pub fn remove_session(&self, id: &str) {
        self.sessions.remove(id);
        tracing::debug!(session_id = %id, "Session removed");
    }

    /// 保存状态到磁盘
    pub async fn save_state(&self) -> Result<()> {
        let sessions: Vec<Session> = self
            .sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        let serialized = serde_json::to_string_pretty(&sessions)?;

        fs::create_dir_all(self.storage_path.parent().unwrap()).await?;
        fs::write(&self.storage_path, serialized).await?;

        tracing::info!(
            count = sessions.len(),
            path = ?self.storage_path,
            "Sessions saved"
        );

        Ok(())
    }

    /// 从磁盘恢复状态
    pub async fn restore_state(&self) -> Result<()> {
        if !self.storage_path.exists() {
            tracing::debug!("No saved state found");
            return Ok(());
        }

        let data = fs::read_to_string(&self.storage_path).await?;
        let sessions: Vec<Session> = serde_json::from_str(&data)?;

        for session in sessions {
            self.sessions.insert(session.id.clone(), session);
        }

        tracing::info!(
            count = self.sessions.len(),
            "Sessions restored"
        );

        Ok(())
    }

    /// 启动自动保存任务
    pub fn start_auto_save(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(self.auto_save_interval);

            loop {
                ticker.tick().await;

                if let Err(e) = self.save_state().await {
                    tracing::error!(error = %e, "Failed to save session state");
                }
            }
        });
    }

    /// 获取所有会话
    pub fn get_all_sessions(&self) -> Vec<Session> {
        self.sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// 清理过期会话
    pub async fn cleanup_expired(&self, max_idle: Duration) {
        let now = SystemTime::now();
        let mut expired = Vec::new();

        for entry in self.sessions.iter() {
            if let Ok(idle_time) = now.duration_since(entry.value().last_active) {
                if idle_time > max_idle {
                    expired.push(entry.key().clone());
                }
            }
        }

        for id in &expired {
            self.sessions.remove(id);
        }

        if !expired.is_empty() {
            tracing::info!(count = expired.len(), "Cleaned up expired sessions");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_session_persistence() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sessions.json");

        let manager = SessionManager::new(path.clone());

        // 创建会话
        manager.create_session("session1".to_string(), "127.0.0.1:1234".to_string());

        // 保存
        manager.save_state().await.unwrap();

        // 创建新管理器并恢复
        let manager2 = SessionManager::new(path);
        manager2.restore_state().await.unwrap();

        assert_eq!(manager2.sessions.len(), 1);
    }
}
