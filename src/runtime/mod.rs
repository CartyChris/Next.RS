//! The embedded JavaScript runtime: a pool of QuickJS workers that run the server bundle.
//!
//! Each worker is one OS thread that owns one engine plus a single-threaded Tokio event
//! loop, so a worker interleaves many in-flight requests (await fetch, timers, streaming)
//! without ever sharing an engine across threads. Requests go to the least-loaded worker.
//!
//! Limits: a per-worker heap cap, a CPU budget per uninterrupted JS slice, and a wall-clock
//! timeout per request. A worker that hits a hard limit is torn down and rebuilt.

mod host;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};
use bytes::Bytes;
use serde::Deserialize;
use tokio::sync::{Notify, mpsc, oneshot};

pub use host::HOST_JS;

/// Stop feeding a response body from JS once this much is queued and unsent.
pub const HIGH_WATER: usize = 1 << 20;
const LOW_WATER: usize = HIGH_WATER / 4;

#[derive(Debug, Clone)]
pub struct Limits {
    pub memory_bytes: usize,
    pub cpu: Duration,
    pub timeout: Duration,
}

pub type InvalidateHook = Box<dyn Fn(&str) + Send + Sync>;

/// Everything a worker needs that outlives a single engine.
pub struct Shared {
    pub code: Arc<str>,
    pub limits: Limits,
    pub env: HashMap<String, String>,
    pub allow_fetch: Vec<String>,
    pub dev: bool,
    pub cache: Mutex<KvCache>,
    #[cfg(feature = "fetch")]
    pub http: reqwest::Client,
    /// Called with every key/tag passed to `invalidate()` (the server evicts cached pages).
    pub on_invalidate: Mutex<Option<InvalidateHook>>,
}

impl Shared {
    pub fn new(
        code: impl Into<Arc<str>>,
        limits: Limits,
        env: HashMap<String, String>,
        allow_fetch: Vec<String>,
        dev: bool,
    ) -> Self {
        Self {
            #[cfg(feature = "fetch")]
            http: http_client(allow_fetch.clone()),
            code: code.into(),
            limits,
            env,
            allow_fetch,
            dev,
            cache: Mutex::new(KvCache::default()),
            on_invalidate: Mutex::new(None),
        }
    }

    /// May server code fetch this host? Patterns: exact host, `*.suffix`, or `*`.
    pub fn fetch_allowed(&self, host: &str) -> bool {
        host_allowed(&self.allow_fetch, host)
    }
}

/// Redirects are re-checked against the allowlist so a 30x cannot escape it.
#[cfg(feature = "fetch")]
fn http_client(allow: Vec<String>) -> reqwest::Client {
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        let host = attempt.url().host_str().unwrap_or_default().to_string();
        if attempt.previous().len() >= 10 {
            attempt.error("too many redirects")
        } else if host_allowed(&allow, &host) {
            attempt.follow()
        } else {
            attempt.error(format!("redirect to `{host}` is not in server.allowFetch"))
        }
    });
    reqwest::Client::builder().user_agent("forge").redirect(policy).build().expect("http client")
}

fn host_allowed(allow: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    allow.iter().any(|p| {
        let p = p.to_ascii_lowercase();
        p == "*" || p == host || p.strip_prefix("*.").is_some_and(|s| host.ends_with(&format!(".{s}")))
    })
}

/// Process-wide TTL cache behind `forge:cache`, with tag invalidation.
#[derive(Debug, Default)]
pub struct KvCache {
    entries: HashMap<String, (String, Instant, Vec<String>)>,
    tags: HashMap<String, HashSet<String>>,
}

impl KvCache {
    const MAX_ENTRIES: usize = 10_000;

    pub fn get(&mut self, key: &str) -> Option<String> {
        match self.entries.get(key) {
            Some((v, until, _)) if *until > Instant::now() => Some(v.clone()),
            Some(_) => {
                self.remove(key);
                None
            }
            None => None,
        }
    }

    pub fn set(&mut self, key: String, value: String, ttl: Duration, tags: Vec<String>) {
        if self.entries.len() >= Self::MAX_ENTRIES && !self.entries.contains_key(&key) {
            let now = Instant::now();
            let expired: Vec<String> =
                self.entries.iter().filter(|(_, e)| e.1 <= now).map(|(k, _)| k.clone()).collect();
            for k in expired {
                self.remove(&k);
            }
            if self.entries.len() >= Self::MAX_ENTRIES {
                if let Some(k) = self.entries.keys().next().cloned() {
                    self.remove(&k);
                }
            }
        }
        self.remove(&key);
        for t in &tags {
            self.tags.entry(t.clone()).or_default().insert(key.clone());
        }
        self.entries.insert(key, (value, Instant::now() + ttl, tags));
    }

    /// Remove `key` and every entry tagged `key`.
    pub fn invalidate(&mut self, key_or_tag: &str) {
        self.remove(key_or_tag);
        for k in self.tags.remove(key_or_tag).unwrap_or_default() {
            self.remove(&k);
        }
    }

    fn remove(&mut self, key: &str) {
        if let Some((_, _, tags)) = self.entries.remove(key) {
            for t in tags {
                if let Some(set) = self.tags.get_mut(&t) {
                    set.remove(key);
                    if set.is_empty() {
                        self.tags.remove(&t);
                    }
                }
            }
        }
    }
}

/// First message of every response.
#[derive(Debug, Clone, Deserialize)]
pub struct Head {
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// Present for rendered pages: the body is the app markup, the host wraps it in a document.
    #[serde(default)]
    pub page: Option<PageHead>,
    /// `"not-found"` when the app has no not-found page.
    #[serde(default)]
    pub builtin: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PageHead {
    #[serde(default)]
    pub meta: serde_json::Map<String, serde_json::Value>,
    /// JSON for the `__FORGE_DATA__` script, already safe to inline.
    pub boot: String,
    /// The route that rendered (the not-found page when the loader said so).
    pub route: String,
    #[serde(default = "yes")]
    pub ssr: bool,
}

fn yes() -> bool {
    true
}

pub(crate) enum Event {
    Head(String),
    Data(Bytes),
}

/// The write side of one response, held by the worker.
#[derive(Clone)]
pub(crate) struct Sink {
    tx: mpsc::UnboundedSender<Event>,
    queued: Arc<AtomicUsize>,
    drained: Arc<Notify>,
}

impl Sink {
    /// 0 = accepted, 1 = accepted but over the high-water mark, 2 = client gone.
    pub(crate) fn send(&self, event: Event) -> u8 {
        let len = match &event {
            Event::Data(b) => b.len(),
            Event::Head(_) => 0,
        };
        let total = self.queued.fetch_add(len, Ordering::AcqRel) + len;
        if self.tx.send(event).is_err() {
            return 2;
        }
        u8::from(total > HIGH_WATER)
    }

    pub(crate) async fn drained(&self) {
        loop {
            let wait = self.drained.notified();
            if self.queued.load(Ordering::Acquire) <= LOW_WATER || self.tx.is_closed() {
                return;
            }
            wait.await;
        }
    }
}

/// The read side: a streaming response body.
pub struct Body {
    rx: mpsc::UnboundedReceiver<Event>,
    queued: Arc<AtomicUsize>,
    drained: Arc<Notify>,
}

impl Body {
    pub async fn next(&mut self) -> Option<Bytes> {
        loop {
            match self.rx.recv().await? {
                Event::Data(b) => {
                    self.queued.fetch_sub(b.len(), Ordering::AcqRel);
                    self.drained.notify_waiters();
                    if !b.is_empty() {
                        return Some(b);
                    }
                }
                Event::Head(_) => {}
            }
        }
    }

    pub async fn collect(mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(b) = self.next().await {
            out.extend_from_slice(&b);
        }
        out
    }

    pub fn into_stream(self) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static {
        futures_util::stream::unfold(self, |mut body| async move { body.next().await.map(|b| (Ok(b), body)) })
    }
}

pub struct Reply {
    pub head: Head,
    pub body: Body,
}

pub(crate) struct Job {
    pub(crate) request: String,
    pub(crate) body: Option<Bytes>,
    pub(crate) sink: Sink,
}

struct Worker {
    tx: mpsc::UnboundedSender<Job>,
    inflight: Arc<AtomicUsize>,
}

pub struct Pool {
    workers: Vec<Worker>,
    pub shared: Arc<Shared>,
}

#[derive(Debug)]
pub enum DispatchError {
    Timeout(Duration),
    NoResponse,
    BadHead(String),
}

impl std::error::Error for DispatchError {}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::Timeout(d) => write!(f, "request timed out after {d:?}"),
            DispatchError::NoResponse => f.write_str("the JS worker ended the request without a response"),
            DispatchError::BadHead(e) => write!(f, "invalid response head: {e}"),
        }
    }
}

impl Pool {
    /// Start `workers` engines and wait until each has loaded the bundle.
    /// Fails with the bundle's top-level error if it cannot load.
    pub async fn start(shared: Arc<Shared>, workers: usize) -> Result<Self> {
        let mut pool = Pool { workers: Vec::with_capacity(workers), shared: shared.clone() };
        let mut ready = Vec::new();
        for i in 0..workers.max(1) {
            let (tx, rx) = mpsc::unbounded_channel();
            let (ready_tx, ready_rx) = oneshot::channel();
            let inflight = Arc::new(AtomicUsize::new(0));
            let (s, f) = (shared.clone(), inflight.clone());
            std::thread::Builder::new()
                .name(format!("forge-js-{i}"))
                .stack_size(16 << 20)
                .spawn(move || host::run_worker(s, rx, f, ready_tx))
                .context("spawning JS worker")?;
            pool.workers.push(Worker { tx, inflight });
            ready.push(ready_rx);
        }
        for r in ready {
            r.await.map_err(|_| anyhow!("JS worker exited during startup"))?.map_err(|e| anyhow!("{e}"))?;
        }
        Ok(pool)
    }

    /// Send one request (a JSON object understood by `runtime/server.js`) and wait for its head.
    pub async fn dispatch(&self, request: String, body: Option<Bytes>) -> Result<Reply, DispatchError> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let queued = Arc::new(AtomicUsize::new(0));
        let drained = Arc::new(Notify::new());
        let sink = Sink { tx, queued: queued.clone(), drained: drained.clone() };
        let worker = self.workers.iter().min_by_key(|w| w.inflight.load(Ordering::Relaxed)).expect("pool has workers");
        if worker.tx.send(Job { request, body, sink }).is_err() {
            return Err(DispatchError::NoResponse);
        }
        let timeout = self.shared.limits.timeout;
        let head = match tokio::time::timeout(timeout, rx.recv()).await {
            Err(_) => return Err(DispatchError::Timeout(timeout)),
            Ok(None) => return Err(DispatchError::NoResponse),
            Ok(Some(Event::Head(h))) => h,
            Ok(Some(Event::Data(_))) => return Err(DispatchError::BadHead("body before head".into())),
        };
        let head: Head = serde_json::from_str(&head).map_err(|e| DispatchError::BadHead(e.to_string()))?;
        Ok(Reply { head, body: Body { rx, queued, drained } })
    }

    pub fn len(&self) -> usize {
        self.workers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.workers.is_empty()
    }

    pub fn inflight(&self) -> usize {
        self.workers.iter().map(|w| w.inflight.load(Ordering::Relaxed)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_cache_ttl_and_tags() {
        let mut c = KvCache::default();
        c.set("a".into(), "1".into(), Duration::from_secs(60), vec!["posts".into()]);
        c.set("b".into(), "2".into(), Duration::from_secs(60), vec!["posts".into(), "x".into()]);
        c.set("c".into(), "3".into(), Duration::ZERO, vec![]);
        assert_eq!(c.get("a").as_deref(), Some("1"));
        assert_eq!(c.get("c"), None);
        c.invalidate("posts");
        assert_eq!(c.get("a"), None);
        assert_eq!(c.get("b"), None);
        assert!(c.tags.is_empty());
    }

    #[test]
    fn fetch_allowlist() {
        let s = Shared::new(
            "",
            Limits { memory_bytes: 0, cpu: Duration::ZERO, timeout: Duration::ZERO },
            HashMap::new(),
            vec!["api.example.com".into(), "*.openai.com".into()],
            false,
        );
        assert!(s.fetch_allowed("api.example.com"));
        assert!(s.fetch_allowed("API.openai.com"));
        assert!(!s.fetch_allowed("openai.com.evil.net"));
        assert!(!s.fetch_allowed("example.com"));
    }
}
