//! Self-signed TLS material for local RSQ testing.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn write_dev_certs(output_dir: &Path, common_name: &str) -> Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("create {}", output_dir.display()))?;

    let sans = vec![
        common_name.to_string(),
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ];
    let certified = rcgen::generate_simple_self_signed(sans).context("generate self-signed cert")?;

    let cert_path = output_dir.join("fullchain.pem");
    let key_path = output_dir.join("privkey.pem");
    std::fs::write(&cert_path, certified.cert.pem()).context("write fullchain.pem")?;
    std::fs::write(&key_path, certified.key_pair.serialize_pem()).context("write privkey.pem")?;

    Ok((cert_path, key_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_pem_files() {
        let dir = std::env::temp_dir().join(format!("rsq-cert-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (cert, key) = write_dev_certs(&dir, "rsq.local").unwrap();
        assert!(cert.is_file());
        assert!(key.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
}
