//! AnyTLS inbound stream handler with G5 user limits and traffic accounting.

use anytls_rs::protocol::{Command, Frame};
use anytls_rs::server::handler::{connect_outbound_tcp, read_socks_addr, StreamHandler};
use anytls_rs::session::{Session, Stream};
use anytls_rs::util::Result as AnyTlsResult;
use rsb_core::SharedConnectionManager;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct UserRelayHandler {
    connections: SharedConnectionManager,
    inbound_tag: String,
    password: String,
}

impl UserRelayHandler {
    pub fn new(
        connections: SharedConnectionManager,
        inbound_tag: String,
        password: String,
    ) -> Self {
        Self {
            connections,
            inbound_tag,
            password,
        }
    }

    fn user_name(&self) -> String {
        self.connections
            .users()
            .lookup_password(&self.password)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| self.password.chars().take(8).collect())
    }

    fn user_limits(&self) -> rsb_core::UserLimits {
        self.connections
            .users()
            .lookup_password(&self.password)
            .map(|r| r.limits.clone())
            .unwrap_or_default()
    }
}

impl StreamHandler for UserRelayHandler {
    fn handle_stream(
        &self,
        stream: Arc<Stream>,
        session: Arc<Session>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AnyTlsResult<()>> + Send + '_>> {
        Box::pin(async move {
            let stream_id = stream.id();
            let peer_version = session.peer_version();

            let destination = read_socks_addr(stream.clone()).await?;

            if destination.addr.contains("udp-over-tcp.arpa") {
                if peer_version >= 2 {
                    let synack_frame = Frame::control(Command::SynAck, stream_id);
                    session.write_control_frame(synack_frame).await?;
                }
                return anytls_rs::server::udp_proxy::handle_udp_over_tcp(stream).await;
            }

            let outbound = connect_outbound_tcp(
                session.clone(),
                stream_id,
                peer_version,
                &destination,
            )
            .await?;

            let dest_addr = resolve_socket_addr(&destination.addr, destination.port).await;
            let domain = if dest_addr.is_none() {
                Some(destination.addr.clone())
            } else {
                None
            };

            let relay_session = crate::inbound_proxy::UserRelaySession::begin(
                self.connections.clone(),
                &self.inbound_tag,
                &self.user_name(),
                self.user_limits(),
                dest_addr,
                domain,
            )
            .map_err(|e| anytls_rs::util::AnyTlsError::Protocol(e.to_string()))?;

            crate::inbound_proxy::relay_anytls_stream_user(&relay_session, stream, outbound)
                .await
                .map_err(|e| anytls_rs::util::AnyTlsError::Protocol(e.to_string()))
        })
    }
}

async fn resolve_socket_addr(host: &str, port: u16) -> Option<SocketAddr> {
    if let Ok(ip) = host.parse() {
        return Some(SocketAddr::new(ip, port));
    }
    tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .ok()
        .and_then(|mut it| it.next())
}
