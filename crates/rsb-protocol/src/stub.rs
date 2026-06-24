use async_trait::async_trait;
use rsb_core::{BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpSocket};

pub struct StubInbound {
    tag: String,
    kind: String,
}

impl StubInbound {
    pub fn new(tag: String, kind: &str) -> Self {
        tracing::warn!(
            tag = %tag,
            kind,
            "inbound type not implemented yet — see FEATURES.md"
        );
        Self {
            tag,
            kind: kind.to_string(),
        }
    }
}

#[async_trait]
impl Inbound for StubInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        &self.kind
    }
    async fn start(&self) -> Result<(), BoxError> {
        tracing::warn!(
            tag = %self.tag,
            kind = %self.kind,
            "inbound stub started (no listener)"
        );
        Ok(())
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct StubOutbound {
    tag: String,
    kind: String,
}

impl StubOutbound {
    pub fn new(tag: String, kind: &str) -> Self {
        tracing::warn!(
            tag = %tag,
            kind,
            "outbound type not implemented yet — see FEATURES.md"
        );
        Self {
            tag,
            kind: kind.to_string(),
        }
    }
}

#[async_trait]
impl Outbound for StubOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        &self.kind
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, _destination: std::net::SocketAddr) -> Result<ProxyConn, BoxError> {
        anyhow::bail!(
            "outbound `{}` (type `{}`) not implemented — see FEATURES.md",
            self.tag,
            self.kind
        )
    }
    async fn dial_udp(&self, _destination: std::net::SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        anyhow::bail!(
            "outbound `{}` (type `{}`) not implemented — see FEATURES.md",
            self.tag,
            self.kind
        )
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}
