// 链式代理实现
use anyhow::Result;
use async_trait::async_trait;
use rsb_core::{BoxError, Network, Outbound, ProxyConn};
use std::net::SocketAddr;
use std::sync::Arc;

pub struct ChainOutbound {
    tag: String,
    outbounds: Vec<Arc<dyn Outbound>>,
}

impl ChainOutbound {
    pub fn new(tag: String, outbounds: Vec<Arc<dyn Outbound>>) -> Self {
        Self { tag, outbounds }
    }

    async fn dial_through_chain(&self, destination: SocketAddr) -> Result<ProxyConn> {
        if self.outbounds.is_empty() {
            anyhow::bail!("No outbounds in chain");
        }

        tracing::debug!(
            chain_length = self.outbounds.len(),
            destination = %destination,
            "Dialing through proxy chain"
        );

        // 第一个代理直接连接
        let mut current = self.outbounds[0].dial_tcp(destination, None).await?;

        // 通过每个代理依次连接
        for (i, outbound) in self.outbounds.iter().skip(1).enumerate() {
            tracing::debug!(
                hop = i + 1,
                outbound = %outbound.tag(),
                "Connecting through proxy chain"
            );

            // 通过前一个代理连接到下一个代理
            current = outbound.dial_tcp(destination, None).await?;
        }

        tracing::info!(
            hops = self.outbounds.len(),
            "Successfully established chain connection"
        );

        Ok(current)
    }
}

#[async_trait]
impl Outbound for ChainOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        "chain"
    }

    fn networks(&self) -> &[Network] {
        // 取所有代理支持的网络类型的交集
        if self.outbounds.is_empty() {
            return &[];
        }

        let first_networks = self.outbounds[0].networks();
        if self.outbounds.len() == 1 {
            return first_networks;
        }

        // 简化实现：如果所有代理都支持 TCP 和 UDP，返回两者
        let all_support_tcp = self.outbounds.iter().all(|ob| {
            ob.networks().contains(&Network::Tcp)
        });

        let all_support_udp = self.outbounds.iter().all(|ob| {
            ob.networks().contains(&Network::Udp)
        });

        if all_support_tcp && all_support_udp {
            &[Network::Tcp, Network::Udp]
        } else if all_support_tcp {
            &[Network::Tcp]
        } else {
            &[]
        }
    }

    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        _domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        self.dial_through_chain(destination).await.map_err(Into::into)
    }

    async fn dial_udp(&self) -> Result<rsb_core::ProxyUdpSocket, BoxError> {
        // 链式代理的 UDP 实现较复杂，需要每一跳都支持
        anyhow::bail!("UDP through chain proxy not implemented yet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_creation() {
        let chain = ChainOutbound::new("chain".to_string(), vec![]);
        assert_eq!(chain.tag(), "chain");
        assert_eq!(chain.kind(), "chain");
    }
}
