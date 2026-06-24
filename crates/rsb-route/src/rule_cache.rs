//! On-disk cache for remote rule-set downloads (geosite/geoip and rule_set URLs).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct RuleSetCache {
    base: PathBuf,
}

impl RuleSetCache {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    pub fn default_path() -> Self {
        Self::new("cache/rule-set")
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.base
    }

    pub fn file_path(&self, tag: &str, binary: bool) -> PathBuf {
        let ext = if binary { "srs" } else { "txt" };
        self.base.join(format!("{}.{}", sanitize_tag(tag), ext))
    }

    pub async fn read_or_fetch(&self, tag: &str, url: &str, binary: bool) -> Result<Vec<u8>> {
        let path = self.file_path(tag, binary);
        let cached = if path.is_file() {
            tokio::fs::read(&path)
                .await
                .with_context(|| format!("read cached rule-set `{}`", path.display()))
                .ok()
        } else {
            None
        };

        match reqwest::get(url).await {
            Ok(resp) => {
                let bytes = resp
                    .bytes()
                    .await
                    .with_context(|| format!("read rule-set body `{url}`"))?
                    .to_vec();
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await.ok();
                }
                if let Err(err) = tokio::fs::write(&path, &bytes).await {
                    tracing::warn!(
                        path = %path.display(),
                        error = %err,
                        "failed to write rule-set cache"
                    );
                } else {
                    tracing::debug!(path = %path.display(), tag, "rule-set cache updated");
                }
                Ok(bytes)
            }
            Err(err) => {
                if let Some(bytes) = cached {
                    tracing::warn!(tag, %url, error = %err, "rule-set fetch failed, using cache");
                    Ok(bytes)
                } else {
                    Err(err).with_context(|| format!("fetch rule-set `{url}`"))
                }
            }
        }
    }
}

fn sanitize_tag(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    for ch in tag.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "ruleset".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_tag_replaces_unsafe_chars() {
        assert_eq!(sanitize_tag("geosite-cn"), "geosite-cn");
        assert_eq!(sanitize_tag("a/b:c"), "a_b_c");
    }
}
