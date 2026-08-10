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

    #[allow(dead_code)]
    pub async fn read_or_fetch(&self, tag: &str, url: &str, binary: bool) -> Result<Vec<u8>> {
        self.read_or_fetch_urls(tag, &[url.to_string()], binary).await
    }

    /// Try each URL in order; prefer on-disk cache when present (bundled / prior extract).
    pub async fn read_or_fetch_urls(
        &self,
        tag: &str,
        urls: &[String],
        binary: bool,
    ) -> Result<Vec<u8>> {
        anyhow::ensure!(!urls.is_empty(), "no rule-set urls for `{tag}`");
        let path = self.file_path(tag, binary);
        if path.is_file() {
            match tokio::fs::read(&path).await {
                Ok(bytes) if bytes.len() >= 64 => {
                    tracing::debug!(
                        path = %path.display(),
                        tag,
                        len = bytes.len(),
                        "using local rule-set cache"
                    );
                    return Ok(bytes);
                },
                Ok(_) => {
                    tracing::warn!(path = %path.display(), tag, "local rule-set too small, refetch");
                },
                Err(err) => {
                    tracing::warn!(path = %path.display(), tag, error = %err, "read local rule-set failed");
                },
            }
        }

        let mut last_err: Option<anyhow::Error> = None;
        for url in urls {
            match reqwest::get(url).await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.bytes().await {
                        Ok(bytes) => {
                            let bytes = bytes.to_vec();
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
                                tracing::debug!(
                                    path = %path.display(),
                                    tag,
                                    %url,
                                    "rule-set cache updated"
                                );
                            }
                            return Ok(bytes);
                        },
                        Err(err) => {
                            last_err = Some(anyhow::Error::new(err).context(format!(
                                "read rule-set body `{url}`"
                            )));
                        },
                    }
                },
                Ok(resp) => {
                    last_err = Some(anyhow::anyhow!(
                        "fetch rule-set `{url}` status {}",
                        resp.status()
                    ));
                },
                Err(err) => {
                    last_err = Some(anyhow::Error::new(err).context(format!("fetch rule-set `{url}`")));
                },
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("fetch rule-set `{tag}` failed")))
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
