// 断点续传实现
use anyhow::Result;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::time::Duration;

pub struct ResumableTransfer {
    checkpoint_interval: Duration,
    checkpoints: Arc<DashMap<String, TransferState>>,
    checkpoint_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferState {
    pub transfer_id: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub checksum: String,
    pub last_checkpoint: std::time::SystemTime,
    pub file_path: String,
}

impl ResumableTransfer {
    pub fn new(checkpoint_dir: PathBuf, checkpoint_interval: Duration) -> Self {
        Self {
            checkpoint_interval,
            checkpoints: Arc::new(DashMap::new()),
            checkpoint_dir,
        }
    }

    /// 开始新的传输
    pub async fn start_transfer(
        &self,
        transfer_id: String,
        file_path: String,
        total_bytes: u64,
    ) -> Result<TransferState> {
        let state = TransferState {
            transfer_id: transfer_id.clone(),
            total_bytes,
            transferred_bytes: 0,
            checksum: String::new(),
            last_checkpoint: std::time::SystemTime::now(),
            file_path,
        };

        self.checkpoints.insert(transfer_id.clone(), state.clone());
        self.save_checkpoint(&transfer_id).await?;

        tracing::info!(
            transfer_id = %transfer_id,
            total_bytes = total_bytes,
            "Transfer started"
        );

        Ok(state)
    }

    /// 从检查点恢复
    pub async fn resume_from_checkpoint(&self, transfer_id: &str) -> Result<Option<TransferState>> {
        // 尝试从内存加载
        if let Some(state) = self.checkpoints.get(transfer_id) {
            tracing::info!(
                transfer_id = %transfer_id,
                progress = state.transferred_bytes,
                total = state.total_bytes,
                "Resumed from memory checkpoint"
            );
            return Ok(Some(state.clone()));
        }

        // 从磁盘加载
        let checkpoint_path = self.checkpoint_path(transfer_id);
        if !checkpoint_path.exists() {
            return Ok(None);
        }

        let data = tokio::fs::read_to_string(&checkpoint_path).await?;
        let state: TransferState = serde_json::from_str(&data)?;

        self.checkpoints.insert(transfer_id.to_string(), state.clone());

        tracing::info!(
            transfer_id = %transfer_id,
            progress = state.transferred_bytes,
            total = state.total_bytes,
            "Resumed from disk checkpoint"
        );

        Ok(Some(state))
    }

    /// 更新传输进度
    pub async fn update_progress(&self, transfer_id: &str, bytes: u64) -> Result<()> {
        if let Some(mut state) = self.checkpoints.get_mut(transfer_id) {
            state.transferred_bytes += bytes;

            let elapsed = state.last_checkpoint.elapsed().unwrap_or_default();
            if elapsed >= self.checkpoint_interval {
                drop(state); // 释放锁
                self.save_checkpoint(transfer_id).await?;
            }
        }

        Ok(())
    }

    /// 保存检查点
    async fn save_checkpoint(&self, transfer_id: &str) -> Result<()> {
        if let Some(mut state) = self.checkpoints.get_mut(transfer_id) {
            state.last_checkpoint = std::time::SystemTime::now();

            let checkpoint_path = self.checkpoint_path(transfer_id);
            tokio::fs::create_dir_all(&self.checkpoint_dir).await?;

            let data = serde_json::to_string(&*state)?;
            tokio::fs::write(&checkpoint_path, data).await?;

            tracing::debug!(
                transfer_id = %transfer_id,
                progress = state.transferred_bytes,
                "Checkpoint saved"
            );
        }

        Ok(())
    }

    /// 完成传输
    pub async fn complete_transfer(&self, transfer_id: &str) -> Result<()> {
        self.checkpoints.remove(transfer_id);

        let checkpoint_path = self.checkpoint_path(transfer_id);
        if checkpoint_path.exists() {
            tokio::fs::remove_file(&checkpoint_path).await?;
        }

        tracing::info!(transfer_id = %transfer_id, "Transfer completed");

        Ok(())
    }

    /// 取消传输
    pub async fn cancel_transfer(&self, transfer_id: &str) -> Result<()> {
        self.checkpoints.remove(transfer_id);

        tracing::info!(transfer_id = %transfer_id, "Transfer cancelled");

        Ok(())
    }

    /// 获取传输进度
    pub fn get_progress(&self, transfer_id: &str) -> Option<f64> {
        self.checkpoints.get(transfer_id).map(|state| {
            if state.total_bytes == 0 {
                0.0
            } else {
                (state.transferred_bytes as f64 / state.total_bytes as f64) * 100.0
            }
        })
    }

    fn checkpoint_path(&self, transfer_id: &str) -> PathBuf {
        self.checkpoint_dir.join(format!("{}.json", transfer_id))
    }

    /// 下载文件（支持断点续传）
    pub async fn download_file(
        &self,
        transfer_id: String,
        url: &str,
        output_path: PathBuf,
    ) -> Result<()> {
        // 检查是否有检查点
        let resume_from = if let Some(state) = self.resume_from_checkpoint(&transfer_id).await? {
            state.transferred_bytes
        } else {
            0
        };

        // 打开文件（追加模式）
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&output_path)
            .await?;

        if resume_from > 0 {
            file.seek(SeekFrom::Start(resume_from)).await?;
            tracing::info!(
                transfer_id = %transfer_id,
                resume_from = resume_from,
                "Resuming download"
            );
        }

        // 发送 HTTP 请求（带 Range 头）
        let client = reqwest::Client::new();
        let mut request = client.get(url);

        if resume_from > 0 {
            request = request.header("Range", format!("bytes={}-", resume_from));
        }

        let mut response = request.send().await?;
        let total_bytes = response.content_length().unwrap_or(0) + resume_from;

        // 创建或更新传输状态
        if resume_from == 0 {
            self.start_transfer(
                transfer_id.clone(),
                output_path.to_string_lossy().to_string(),
                total_bytes,
            )
            .await?;
        }

        // 下载数据
        let mut buffer = vec![0u8; 64 * 1024]; // 64KB buffer

        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            self.update_progress(&transfer_id, chunk.len() as u64).await?;
        }

        file.flush().await?;

        self.complete_transfer(&transfer_id).await?;

        tracing::info!(
            transfer_id = %transfer_id,
            total_bytes = total_bytes,
            "Download completed"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_resumable_transfer() {
        let dir = tempdir().unwrap();
        let transfer = ResumableTransfer::new(
            dir.path().to_path_buf(),
            Duration::from_secs(1),
        );

        // 开始传输
        let state = transfer
            .start_transfer(
                "test-transfer".to_string(),
                "test.bin".to_string(),
                1024 * 1024,
            )
            .await
            .unwrap();

        assert_eq!(state.transferred_bytes, 0);

        // 更新进度
        transfer.update_progress("test-transfer", 1024).await.unwrap();

        // 获取进度
        let progress = transfer.get_progress("test-transfer");
        assert!(progress.is_some());
    }
}
