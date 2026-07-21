//! Shared QUIC/TLS settings for RSQ.

use anyhow::{Context, Result};
use quinn::{ClientConfig, ServerConfig as QuinnServerConfig, TransportConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use std::time::Duration;

pub const ALPN_RSQ: &[u8] = b"rsq/1";

pub const DEFAULT_STREAM_RECV_WINDOW: u32 = 8 * 1024 * 1024;
pub const DEFAULT_CONN_RECV_WINDOW: u32 = DEFAULT_STREAM_RECV_WINDOW * 5 / 2;

fn apply_brutal_transport(transport: &mut TransportConfig, brutal_bps: u64) {
    let brutal = super::brutal::BrutalConfig::new(brutal_bps);
    transport.congestion_controller_factory(Arc::new(brutal));
    transport.send_window((DEFAULT_CONN_RECV_WINDOW as u64).into());
    transport.send_fairness(false);
    transport.enable_segmentation_offload(false);
}

fn apply_client_mtu(transport: &mut TransportConfig, disable_mtu_discovery: bool) {
    if disable_mtu_discovery {
        transport.mtu_discovery_config(None);
    } else {
        // 对齐 Hy2：允许 PMTU（含 Windows），避免写死 1200。
        transport.mtu_discovery_config(Some(quinn::MtuDiscoveryConfig::default()));
    }
}

pub fn client_tls(insecure: bool) -> rustls::ClientConfig {
    if insecure {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(crate::transport::SkipVerifier))
            .with_no_client_auth()
    } else {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    }
}

pub fn build_client_config(
    tls: rustls::ClientConfig,
    profile: Option<super::traffic::TrafficProfile>,
    up_mbps: u32,
    down_mbps: u32,
    idle_timeout: Option<Duration>,
    keep_alive_period: Option<Duration>,
    use_brutal: bool,
    disable_mtu_discovery: bool,
) -> Result<ClientConfig> {
    let mut tls = tls;
    tls.alpn_protocols = vec![ALPN_RSQ.to_vec()];
    let mut transport = TransportConfig::default();
    let keepalive = keep_alive_period.unwrap_or_else(|| {
        profile
            .map(|p| super::traffic::jitter_duration(p.keepalive_jitter_base_secs()))
            .unwrap_or_else(|| Duration::from_secs(10))
    });
    transport.keep_alive_interval(Some(keepalive));
    let idle = idle_timeout.unwrap_or(Duration::from_secs(30));
    transport.max_idle_timeout(Some(
        idle.try_into()
            .map_err(|e| anyhow::anyhow!("idle timeout: {e}"))?,
    ));
    transport.stream_receive_window(DEFAULT_STREAM_RECV_WINDOW.into());
    transport.receive_window(DEFAULT_CONN_RECV_WINDOW.into());
    transport.max_concurrent_bidi_streams(256u32.into());
    transport.max_concurrent_uni_streams(256u32.into());
    apply_client_mtu(&mut transport, disable_mtu_discovery);
    if use_brutal {
        apply_brutal_transport(
            &mut transport,
            super::brutal::brutal_bps_from_pair(up_mbps, down_mbps),
        );
    }

    let mut client_cfg = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls)?,
    ));
    client_cfg.transport_config(Arc::new(transport));
    Ok(client_cfg)
}

pub fn build_server_config(
    cert_path: &str,
    key_path: &str,
    up_mbps: u32,
    down_mbps: u32,
) -> Result<QuinnServerConfig> {
    let cert_chain = load_certs(cert_path)?;
    let key = load_key(key_path)?;
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .context("build tls config")?;
    server_crypto.alpn_protocols = vec![ALPN_RSQ.to_vec()];

    let mut transport = TransportConfig::default();
    transport.max_concurrent_bidi_streams(256u32.into());
    transport.stream_receive_window(DEFAULT_STREAM_RECV_WINDOW.into());
    transport.receive_window(DEFAULT_CONN_RECV_WINDOW.into());
    transport.max_idle_timeout(Some(
        Duration::from_secs(30)
            .try_into()
            .context("idle timeout")?,
    ));
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    // 服务端仍可用 Brutal（吞吐）；客户端默认关。
    apply_brutal_transport(
        &mut transport,
        super::brutal::brutal_bps_from_pair(up_mbps, down_mbps),
    );

    let mut server_config = QuinnServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
            .context("quic server crypto")?,
    ));
    server_config.transport_config(Arc::new(transport));
    Ok(server_config)
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path).with_context(|| format!("open cert {path}"))?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .context("read cert pem")
}

fn load_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let file = std::fs::File::open(path).with_context(|| format!("open key {path}"))?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)
        .context("read key pem")?
        .ok_or_else(|| anyhow::anyhow!("no private key found in {path}"))
}
