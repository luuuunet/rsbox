// 流量混淆增强实现
use anyhow::Result;
use bytes::{Buf, BufMut, BytesMut};
use rand::Rng;
use std::time::Duration;
use tokio::time::sleep;

pub struct TrafficObfuscator {
    padding_enabled: bool,
    random_padding: bool,
    min_padding: usize,
    max_padding: usize,
    timing_obfuscation: bool,
    max_delay_ms: u64,
}

impl TrafficObfuscator {
    pub fn new() -> Self {
        Self {
            padding_enabled: true,
            random_padding: true,
            min_padding: 16,
            max_padding: 256,
            timing_obfuscation: true,
            max_delay_ms: 50,
        }
    }

    pub fn with_padding(mut self, enabled: bool, min: usize, max: usize) -> Self {
        self.padding_enabled = enabled;
        self.min_padding = min;
        self.max_padding = max;
        self
    }

    pub fn with_timing_obfuscation(mut self, enabled: bool, max_delay_ms: u64) -> Self {
        self.timing_obfuscation = enabled;
        self.max_delay_ms = max_delay_ms;
        self
    }

    /// 混淆数据
    pub async fn obfuscate(&self, data: &mut BytesMut) -> Result<()> {
        // 1. 添加填充
        if self.padding_enabled {
            self.add_padding(data)?;
        }

        // 2. 时序混淆
        if self.timing_obfuscation {
            self.apply_timing_obfuscation().await;
        }

        Ok(())
    }

    /// 添加填充数据
    fn add_padding(&self, data: &mut BytesMut) -> Result<()> {
        let mut rng = rand::thread_rng();

        let padding_len = if self.random_padding {
            rng.gen_range(self.min_padding..=self.max_padding)
        } else {
            self.max_padding
        };

        // 添加填充长度标记（2 字节）
        let original_len = data.len();
        data.put_u16(padding_len as u16);

        // 添加随机填充
        for _ in 0..padding_len {
            data.put_u8(rng.gen());
        }

        tracing::trace!(
            original_len = original_len,
            padding_len = padding_len,
            total_len = data.len(),
            "Added padding to packet"
        );

        Ok(())
    }

    /// 移除填充数据
    pub fn remove_padding(&self, data: &mut BytesMut) -> Result<()> {
        if data.len() < 2 {
            return Ok(());
        }

        // 读取填充长度
        let padding_len = data.get_u16() as usize;

        // 移除填充
        if data.len() >= padding_len {
            data.advance(padding_len);
        }

        Ok(())
    }

    /// 时序混淆（随机延迟）
    async fn apply_timing_obfuscation(&self) {
        let mut rng = rand::thread_rng();
        let delay_ms = rng.gen_range(0..=self.max_delay_ms);

        if delay_ms > 0 {
            sleep(Duration::from_millis(delay_ms)).await;
            tracing::trace!(delay_ms = delay_ms, "Applied timing obfuscation");
        }
    }

    /// 流量分片
    pub fn fragment(&self, data: &[u8], max_fragment_size: usize) -> Vec<Vec<u8>> {
        let mut fragments = Vec::new();
        let mut offset = 0;
        let mut rng = rand::thread_rng();

        while offset < data.len() {
            // 随机分片大小
            let size = if self.random_padding {
                rng.gen_range(max_fragment_size / 2..=max_fragment_size)
            } else {
                max_fragment_size
            };

            let end = (offset + size).min(data.len());
            fragments.push(data[offset..end].to_vec());
            offset = end;
        }

        tracing::debug!(
            total_size = data.len(),
            fragments = fragments.len(),
            "Fragmented traffic"
        );

        fragments
    }

    /// 组装分片
    pub fn reassemble(&self, fragments: Vec<Vec<u8>>) -> Vec<u8> {
        let total_size: usize = fragments.iter().map(|f| f.len()).sum();
        let mut result = Vec::with_capacity(total_size);

        for fragment in fragments {
            result.extend_from_slice(&fragment);
        }

        tracing::debug!(
            fragments = fragments.len(),
            total_size = total_size,
            "Reassembled fragments"
        );

        result
    }
}

impl Default for TrafficObfuscator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_padding() {
        let obfuscator = TrafficObfuscator::new();
        let mut data = BytesMut::from(&b"Hello, World!"[..]);
        let original_len = data.len();

        obfuscator.obfuscate(&mut data).await.unwrap();

        // 数据应该变长了
        assert!(data.len() > original_len);
    }

    #[test]
    fn test_fragmentation() {
        let obfuscator = TrafficObfuscator::new();
        let data = b"This is a test message that will be fragmented";

        let fragments = obfuscator.fragment(data, 10);
        assert!(fragments.len() > 1);

        let reassembled = obfuscator.reassemble(fragments);
        assert_eq!(reassembled, data);
    }
}
