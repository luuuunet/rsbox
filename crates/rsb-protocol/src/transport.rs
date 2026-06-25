//! Shared TLS / address helpers for protocol outbounds.

use anyhow::{Context, Result};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

pub use crate::utls::TlsIo;

pub async fn tcp_connect(server: &str, port: u16) -> Result<TcpStream> {
    let addr: SocketAddr = tokio::net::lookup_host(format!("{server}:{port}"))
        .await
        .with_context(|| format!("resolve {server}:{port}"))?
        .next()
        .with_context(|| format!("no address for {server}:{port}"))?;
    TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect {server}:{port}"))
}

pub fn build_tls_config(raw: Option<&Value>, insecure: bool) -> Result<Arc<ClientConfig>> {
    let mut cfg = if insecure {
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipVerifier))
            .with_no_client_auth()
    } else {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    };
    if let Some(tls) = raw {
        if let Some(alpn) = tls.get("alpn").and_then(|v| v.as_array()) {
            let protos: Vec<Vec<u8>> = alpn
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.as_bytes().to_vec()))
                .collect();
            if !protos.is_empty() {
                cfg.alpn_protocols = protos;
            }
        }
        crate::utls::apply_fingerprint(&mut cfg, raw);
    }
    Ok(Arc::new(cfg))
}

pub async fn tls_connect(
    server: &str,
    port: u16,
    tls: Option<&Value>,
    sni: Option<&str>,
) -> Result<TlsIo> {
    if crate::reality::is_reality(tls) {
        return crate::reality::connect(server, port, tls, sni).await;
    }
    if crate::utls::utls_enabled(tls) {
        return crate::utls::connect(server, port, tls, sni).await;
    }
    tls_connect_plain(server, port, tls, sni).await
}

pub(crate) async fn tls_connect_plain(
    server: &str,
    port: u16,
    tls: Option<&Value>,
    sni: Option<&str>,
) -> Result<TlsIo> {
    let insecure = tls
        .and_then(|t| t.get("insecure"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let server_name = sni
        .map(str::to_string)
        .or_else(|| {
            tls.and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| server.to_string());
    let cfg = build_tls_config(tls, insecure)?;
    let tcp = tcp_connect(server, port).await?;
    let name = ServerName::try_from(server_name.as_str())
        .map_err(|_| anyhow::anyhow!("invalid sni: {server_name}"))?
        .to_owned();
    Ok(TlsIo::Rustls(
        TlsConnector::from(cfg)
            .connect(name, tcp)
            .await
            .context("tls handshake")?,
    ))
}

#[derive(Debug)]
pub struct SkipVerifier;

impl rustls::client::danger::ServerCertVerifier for SkipVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

pub fn encode_socks_address(addr: SocketAddr) -> Vec<u8> {
    match addr {
        SocketAddr::V4(v4) => {
            let mut buf = vec![0x01];
            buf.extend_from_slice(&v4.ip().octets());
            buf.extend_from_slice(&v4.port().to_be_bytes());
            buf
        },
        SocketAddr::V6(v6) => {
            let mut buf = vec![0x04];
            buf.extend_from_slice(&v6.ip().octets());
            buf.extend_from_slice(&v6.port().to_be_bytes());
            buf
        },
    }
}

pub fn encode_vless_address(dest: SocketAddr) -> Vec<u8> {
    match dest {
        SocketAddr::V4(v4) => {
            let mut buf = vec![0x01];
            buf.extend_from_slice(&v4.ip().octets());
            buf
        },
        SocketAddr::V6(v6) => {
            let mut buf = vec![0x03];
            buf.extend_from_slice(&v6.ip().octets());
            buf
        },
    }
}

pub fn address_from_socket(addr: SocketAddr) -> Vec<u8> {
    encode_socks_address(addr)
}

pub fn sha224_hex(data: &str) -> String {
    use sha2::{Digest, Sha224};
    let hash = Sha224::digest(data.as_bytes());
    hash.iter().map(|b| format!("{b:02x}")).collect()
}
