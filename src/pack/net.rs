//! HTTP for resources that are not npm packages, with an on-disk cache.
//!
//! A packed page is a file people share, and the page being packed may not be trusted. So by
//! default nothing is fetched from private networks: loopback, RFC 1918, link-local (cloud
//! metadata at 169.254.169.254), unique-local, CGNAT, or names like `localhost` and
//! `*.internal`, checked for every redirect hop too. `--allow-private-network` lifts it.

use std::{net::IpAddr, path::PathBuf, time::Duration};

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
    guard: Guard,
}

/// Loopback, private, link-local, unique-local, CGNAT, unspecified and broadcast addresses.
pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, ..] = v.octets();
            v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || a == 0
                || (a == 100 && (b & 0xC0) == 64)
        }
        IpAddr::V6(v) => {
            let first = v.segments()[0];
            v.is_loopback()
                || v.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || v.to_ipv4_mapped().is_some_and(|m| is_private(IpAddr::V4(m)))
        }
    }
}

fn local_name(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    h == "localhost" || [".localhost", ".local", ".internal", ".lan", ".home.arpa"].iter().any(|s| h.ends_with(s))
}

/// Is this URL's host private by its literal form alone (an IP or a local name)?
fn literally_private(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => is_private(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_private(IpAddr::V6(ip)),
        Some(url::Host::Domain(d)) => local_name(d),
        None => false,
    }
}

/// Which hosts a pack may fetch from.
#[derive(Clone)]
pub struct Guard {
    pub allow_private: bool,
    /// Hosts the user configured themselves (the npm registry).
    pub exempt: Vec<String>,
}

impl Guard {
    fn exempt(&self, url: &Url) -> bool {
        self.allow_private || url.host_str().is_some_and(|h| self.exempt.iter().any(|e| e.eq_ignore_ascii_case(h)))
    }

    pub async fn check(&self, url: &Url) -> Result<()> {
        if self.exempt(url) {
            return Ok(());
        }
        let refuse = || {
            anyhow::anyhow!(
                "refusing to fetch from a private network address ({url}); pass --allow-private-network to allow it"
            )
        };
        if literally_private(url) {
            return Err(refuse());
        }
        // A public-looking name may still point inside. Where local DNS cannot answer (some
        // proxies resolve names themselves) the proxy decides.
        if let (Some(url::Host::Domain(d)), Some(port)) = (url.host(), url.port_or_known_default()) {
            if let Ok(Ok(addrs)) =
                tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((d, port))).await
            {
                if addrs.into_iter().any(|a| is_private(a.ip())) {
                    return Err(refuse());
                }
            }
        }
        Ok(())
    }
}

pub fn client(guard: &Guard) -> Result<reqwest::Client> {
    let guard = guard.clone();
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 10 {
            attempt.error("too many redirects")
        } else if !guard.exempt(attempt.url()) && literally_private(attempt.url()) {
            let to = attempt.url().to_string();
            attempt.error(format!(
                "redirect to a private network address ({to}); pass --allow-private-network to allow it"
            ))
        } else {
            attempt.follow()
        }
    });
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .gzip(true)
        .redirect(policy)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .build()?)
}

impl Http {
    pub fn new(client: reqwest::Client, cache: PathBuf, offline: bool, guard: Guard) -> Self {
        Self { client, cache, offline, guard }
    }

    pub async fn get(&self, url: &Url) -> Result<Fetched> {
        // Checked before the cache too: a run that allowed private addresses must not leak
        // what it fetched into one that does not.
        self.guard.check(url).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_private(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "104.16.0.1", "2606:4700::1", "100.128.0.1", "172.32.0.1"] {
            assert!(!is_private(ip.parse().unwrap()), "{ip}");
        }
        for u in [
            "http://localhost:3000/x",
            "http://db.internal/x",
            "http://printer.local/",
            "http://[::1]/",
            "http://169.254.169.254/latest/meta-data/",
        ] {
            assert!(literally_private(&Url::parse(u).unwrap()), "{u}");
        }
        assert!(!literally_private(&Url::parse("https://unpkg.com/x").unwrap()));
    }

    #[tokio::test]
    async fn the_guard_refuses_private_hosts_unless_allowed() {
        let strict = Guard { allow_private: false, exempt: vec!["127.0.0.1".into()] };
        let metadata = Url::parse("http://169.254.169.254/latest/meta-data/iam/").unwrap();
        assert!(strict.check(&metadata).await.unwrap_err().to_string().contains("--allow-private-network"));
        assert!(strict.check(&Url::parse("http://localhost:8080/").unwrap()).await.is_err());
        // The configured registry is the user's choice.
        assert!(strict.check(&Url::parse("http://127.0.0.1:4873/react").unwrap()).await.is_ok());
        let open = Guard { allow_private: true, exempt: vec![] };
        assert!(open.check(&metadata).await.is_ok());
    }
}
