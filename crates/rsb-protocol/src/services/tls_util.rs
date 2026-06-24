//! TLS server config loader (PEM cert/key or rcgen self-signed).

use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use std::path::Path;
use std::sync::Arc;

pub fn load_server_config(
    cert_path: Option<&str>,
    key_path: Option<&str>,
) -> Result<Arc<ServerConfig>> {
    if let (Some(cert), Some(key)) = (cert_path, key_path) {
        return load_pem_pair(cert, key);
    }
    generate_ephemeral()
}

fn load_pem_pair(cert_path: &str, key_path: &str) -> Result<Arc<ServerConfig>> {
    let cert_file =
        std::fs::File::open(cert_path).with_context(|| format!("open cert `{cert_path}`"))?;
    let key_file =
        std::fs::File::open(key_path).with_context(|| format!("open key `{key_path}`"))?;
    let mut cert_reader = std::io::BufReader::new(cert_file);
    let mut key_reader = std::io::BufReader::new(key_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("parse cert pem")?;
    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("parse key pem")?
        .context("missing private key")?;
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("build tls server config")?;
    Ok(Arc::new(cfg))
}

fn generate_ephemeral() -> Result<Arc<ServerConfig>> {
    let cert = rcgen::generate_simple_self_signed(vec!["derp.local".into()])
        .context("generate self-signed cert")?;
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("ephemeral tls config")?;
    Ok(Arc::new(cfg))
}

pub fn tls_enabled(raw: &serde_json::Value) -> bool {
    raw.get("tls")
        .and_then(|t| t.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| raw.get("cert_path").is_some() || raw.get("certificate_path").is_some())
}

pub fn tls_paths(raw: &serde_json::Value) -> (Option<String>, Option<String>) {
    let tls = raw.get("tls");
    let cert = tls
        .and_then(|t| t.get("certificate_path").or_else(|| t.get("cert_path")))
        .or_else(|| raw.get("cert_path").or_else(|| raw.get("certificate_path")))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let key = tls
        .and_then(|t| t.get("key_path").or_else(|| t.get("private_key_path")))
        .or_else(|| raw.get("key_path").or_else(|| raw.get("private_key_path")))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    (cert, key)
}

pub fn write_default_cert_dir(dir: &Path) -> Result<(String, String)> {
    std::fs::create_dir_all(dir)?;
    let cert = dir.join("derp.crt");
    let key = dir.join("derp.key");
    if !cert.exists() {
        let generated = rcgen::generate_simple_self_signed(vec!["derp.local".into()])
            .context("generate derp cert")?;
        std::fs::write(&cert, generated.cert.pem())?;
        std::fs::write(&key, generated.key_pair.serialize_pem())?;
    }
    Ok((cert.display().to_string(), key.display().to_string()))
}
