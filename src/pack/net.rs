//! HTTP for resources that are not npm packages, with an on-disk cache.

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use url::Url;

const MAX_BYTES: usize = 64 << 20;
const TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Font CSS services pick formats by browser; ask as a current browser to get WOFF2.
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub struct Fetched {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    /// After redirects: relative references resolve against this.
    pub url: Url,
}

#[derive(Serialize, Deserialize)]
struct Meta {
    url: String,
    content_type: Option<String>,
}

pub struct Http {
    client: reqwest::Client,
    cache: PathBuf,
    offline: bool,
}

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .gzip(true)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .build()?)
}

impl Http {
    pub fn new(client: reqwest::Client, cache: PathBuf, offline: bool) -> Self {
        Self { client, cache, offline }
    }

    pub async fn get(&self, url: &Url) -> Result<Fetched> {
        let key = blake3::hash(url.as_str().as_bytes()).to_hex()[..24].to_string();
        let (body, meta) = (self.cache.join(format!("{key}.body")), self.cache.join(format!("{key}.json")));
        let fresh = std::fs::metadata(&body)
            .and_then(|m| m.modified())
            .is_ok_and(|t| self.offline || t.elapsed().unwrap_or(TTL) < TTL);
        if fresh {
            if let (Ok(bytes), Ok(m)) = (std::fs::read(&body), std::fs::read(&meta)) {
                if let Ok(m) = serde_json::from_slice::<Meta>(&m) {
                    return Ok(Fetched { bytes, content_type: m.content_type, url: Url::parse(&m.url)? });
                }
            }
        }
        if self.offline {
            bail!("{url} is not in the download cache (offline)");
        }
        let mut res = self.client.get(url.clone()).send().await.with_context(|| format!("fetching {url}"))?;
        if !res.status().is_success() {
            bail!("{url}: HTTP {}", res.status());
        }
        let final_url = res.url().clone();
        let content_type = res.headers().get("content-type").and_then(|v| v.to_str().ok()).map(String::from);
        let mut bytes = Vec::new();
        while let Some(chunk) = res.chunk().await.with_context(|| format!("reading {url}"))? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() > MAX_BYTES {
                bail!("{url}: larger than {} MB", MAX_BYTES >> 20);
            }
        }
        std::fs::create_dir_all(&self.cache)?;
        std::fs::write(&body, &bytes)?;
        std::fs::write(
            &meta,
            serde_json::to_vec(&Meta { url: final_url.to_string(), content_type: content_type.clone() })?,
        )?;
        Ok(Fetched { bytes, content_type, url: final_url })
    }
}
