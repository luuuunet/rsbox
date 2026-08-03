//! Shared QUIC/TLS for RST — ALPN `h3`, Brutal congestion (RSQ).

use anyhow::{Context, Result};
use quinn::{ClientConfig, ServerConfig as QuinnServerConfig, TransportConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use std::time::Duration;

pub const ALPN_H3: &[u8] = b"h3";

pub const DEFAULT_STREAM_RECV_WINDOW: u32 = 8 * 1024 * 1024;
pub const DEFAULT_CONN_RECV_WINDOW: u32 = DEFAULT_STREAM_RECV_WINDOW * 5 / 2;

fn apply_brutal_transport(transport: &mut TransportConfig, brutal_bps: u64) {
    let brutal = crate::rsq::brutal::BrutalConfig::new(brutal_bps);
    transport.congestion_controller_factory(Arc::new(brutal));
    transport.send_window((DEFAULT_CONN_RECV_WINDOW as u64).into());
    transport.send_fairness(false);
    transport.enable_segmentation_offload(false);
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
    up_mbps: u32,
    down_mbps: u32,
    idle_timeout: Duration,
    keep_alive_period: Duration,
    use_brutal: bool,
) -> Result<ClientConfig> {
    let mut tls = tls;
    tls.alpn_protocols = vec![ALPN_H3.to_vec()];
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(keep_alive_period));
    transport.max_idle_timeout(Some(
        idle_timeout
            .try_into()
            .map_err(|e| anyhow::anyhow!("idle timeout: {e}"))?,
    ));
    transport.stream_receive_window(DEFAULT_STREAM_RECV_WINDOW.into());
    transport.receive_window(DEFAULT_CONN_RECV_WINDOW.into());
    transport.max_concurrent_bidi_streams(1024u32.into());
    transport.max_concurrent_uni_streams(1024u32.into());
    transport.mtu_discovery_config(Some(quinn::MtuDiscoveryConfig::default()));
    if use_brutal {
        apply_brutal_transport(
            &mut transport,
            crate::rsq::brutal::brutal_bps_from_pair(up_mbps, down_mbps),
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
    server_crypto.alpn_protocols = vec![ALPN_H3.to_vec()];

    let mut transport = TransportConfig::default();
    transport.max_concurrent_bidi_streams(1024u32.into());
    transport.max_concurrent_uni_streams(1024u32.into());
    transport.stream_receive_window(DEFAULT_STREAM_RECV_WINDOW.into());
    transport.receive_window(DEFAULT_CONN_RECV_WINDOW.into());
    transport.max_idle_timeout(Some(
        Duration::from_secs(60)
            .try_into()
            .context("idle timeout")?,
    ));
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    let brutal_mbps = up_mbps.max(down_mbps).clamp(1, 200);
    apply_brutal_transport(
        &mut transport,
        crate::rsq::brutal::brutal_bps_from_mbps(brutal_mbps),
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
