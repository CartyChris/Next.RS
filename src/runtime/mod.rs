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
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use std::hash::{Hash, Hasher};

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
    /// Open WebSockets and their topic subscriptions, across all workers.
    pub sockets: Sockets,
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
            sockets: Sockets::default(),
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
    // App `fetch()` sees bodies exactly as served (no transparent decompression).
    reqwest::Client::builder().user_agent("forge").redirect(policy).no_gzip().build().expect("http client")
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
    /// `cache()` misses being computed, by key, until when (any worker).
    leases: HashMap<String, Instant>,
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
        self.leases.remove(&key);
        for t in &tags {
            self.tags.entry(t.clone()).or_default().insert(key.clone());
        }
        self.entries.insert(key, (value, Instant::now() + ttl, tags));
    }

    /// Claim the right to compute `key` for up to `ttl`. False while another caller (on any
    /// worker) holds it; it ends when the value is set, the holder releases it, or it expires.
    pub fn lease(&mut self, key: &str, ttl: Duration) -> bool {
        let now = Instant::now();
        if self.leases.get(key).is_some_and(|until| *until > now) {
            return false;
        }
        if self.leases.len() >= Self::MAX_ENTRIES {
            self.leases.retain(|_, until| *until > now);
        }
        self.leases.insert(key.to_string(), now + ttl);
        true
    }

    pub fn release(&mut self, key: &str) {
        self.leases.remove(key);
    }

    /// Live keys starting with `prefix`, sorted.
    pub fn keys(&self, prefix: &str) -> Vec<String> {
        let now = Instant::now();
        let mut keys: Vec<String> =
            self.entries.iter().filter(|(k, e)| k.starts_with(prefix) && e.1 > now).map(|(k, _)| k.clone()).collect();
        keys.sort();
        keys
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
    /// Dropping the body before the response ends aborts the request in JS.
    guard: Option<AbortGuard>,
}

impl Body {
    pub async fn next(&mut self) -> Option<Bytes> {
        loop {
            let Some(event) = self.rx.recv().await else {
                if let Some(g) = &mut self.guard {
                    g.armed = false;
                }
                return None;
            };
            match event {
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

pub(crate) enum Job {
    Request {
        id: u64,
        request: String,
        body: Option<Bytes>,
        sink: Sink,
    },
    /// The client went away or the request timed out.
    Abort(u64),
    Socket(SocketJob),
}

pub(crate) enum SocketJob {
    Open { sid: u64, request: String },
    Message { sid: u64, data: WsData },
    Close { sid: u64, code: u16, reason: String },
}

#[derive(Debug, Clone)]
pub enum WsData {
    Text(String),
    Binary(Bytes),
}

#[derive(Debug)]
pub enum WsOut {
    Data(WsData),
    Close(u16, String),
}

/// Outgoing messages a client may have queued before it counts as too slow and is disconnected.
pub const SOCKET_QUEUE: usize = 1024;

struct SocketEntry {
    tx: mpsc::Sender<WsOut>,
    worker: usize,
    topics: HashSet<String>,
}

/// Every open WebSocket, so any worker (or any HTTP request) can reach any socket.
#[derive(Default)]
pub struct Sockets {
    entries: Mutex<HashMap<u64, SocketEntry>>,
    topics: Mutex<HashMap<String, HashSet<u64>>>,
}

impl Sockets {
    pub fn register(&self, sid: u64, worker: usize, tx: mpsc::Sender<WsOut>) {
        self.entries.lock().unwrap().insert(sid, SocketEntry { tx, worker, topics: HashSet::new() });
    }

    pub fn unregister(&self, sid: u64) {
        let Some(entry) = self.entries.lock().unwrap().remove(&sid) else { return };
        let mut topics = self.topics.lock().unwrap();
        for t in entry.topics {
            if let Some(set) = topics.get_mut(&t) {
                set.remove(&sid);
                if set.is_empty() {
                    topics.remove(&t);
                }
            }
        }
    }

    /// Queue a message. A client whose queue is full is too slow and gets disconnected.
    pub fn send(&self, sid: u64, out: WsOut) -> bool {
        let entries = self.entries.lock().unwrap();
        let Some(entry) = entries.get(&sid) else { return false };
        match entry.tx.try_send(out) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                let tx = entry.tx.clone();
                tokio::spawn(async move {
                    let _ = tx.send(WsOut::Close(1008, "client too slow".into())).await;
                });
                false
            }
            Err(_) => false,
        }
    }

    pub fn subscribe(&self, sid: u64, topic: &str) {
        let mut entries = self.entries.lock().unwrap();
        let Some(entry) = entries.get_mut(&sid) else { return };
        entry.topics.insert(topic.to_string());
        self.topics.lock().unwrap().entry(topic.to_string()).or_default().insert(sid);
    }

    pub fn unsubscribe(&self, sid: u64, topic: &str) {
        if let Some(entry) = self.entries.lock().unwrap().get_mut(&sid) {
            entry.topics.remove(topic);
        }
        let mut topics = self.topics.lock().unwrap();
        if let Some(set) = topics.get_mut(topic) {
            set.remove(&sid);
            if set.is_empty() {
                topics.remove(topic);
            }
        }
    }

    /// Send to every subscriber of `topic` (optionally except one). Returns how many were reached.
    pub fn publish(&self, topic: &str, data: WsData, except: Option<u64>) -> usize {
        let targets: Vec<u64> = match self.topics.lock().unwrap().get(topic) {
            Some(set) => set.iter().copied().filter(|s| Some(*s) != except).collect(),
            None => return 0,
        };
        targets.into_iter().filter(|sid| self.send(*sid, WsOut::Data(data.clone()))).count()
    }

    pub fn subscribers(&self, topic: &str) -> usize {
        self.topics.lock().unwrap().get(topic).map_or(0, HashSet::len)
    }

    pub fn count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    /// Close every socket, or every socket owned by one worker.
    pub fn close_all(&self, worker: Option<usize>, code: u16, reason: &str) {
        for entry in self.entries.lock().unwrap().values() {
            if worker.is_none_or(|w| w == entry.worker) {
                let _ = entry.tx.try_send(WsOut::Close(code, reason.to_string()));
            }
        }
    }
}

/// Aborts the request in JS if the response is dropped before it finished.
struct AbortGuard {
    tx: mpsc::UnboundedSender<Job>,
    id: u64,
    armed: bool,
}

impl Drop for AbortGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.tx.send(Job::Abort(self.id));
        }
    }
}

struct Worker {
    tx: mpsc::UnboundedSender<Job>,
    inflight: Arc<AtomicUsize>,
}

pub struct Pool {
    workers: Vec<Worker>,
    pub shared: Arc<Shared>,
    next_id: AtomicU64,
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
        let mut pool =
            Pool { workers: Vec::with_capacity(workers), shared: shared.clone(), next_id: AtomicU64::new(1) };
        let mut ready = Vec::new();
        for i in 0..workers.max(1) {
            let (tx, rx) = mpsc::unbounded_channel();
            let (ready_tx, ready_rx) = oneshot::channel();
            let inflight = Arc::new(AtomicUsize::new(0));
            let (s, f) = (shared.clone(), inflight.clone());
            std::thread::Builder::new()
                .name(format!("forge-js-{i}"))
                .stack_size(16 << 20)
                .spawn(move || host::run_worker(i, s, rx, f, ready_tx))
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
    /// Dropping the returned future or the reply body before the response ends aborts the
    /// request in JS (its `ctx.signal` fires).
    pub async fn dispatch(&self, request: String, body: Option<Bytes>) -> Result<Reply, DispatchError> {
        self.dispatch_on(None, request, body).await
    }

    /// Like [`Pool::dispatch`], on a specific worker (`None`: the least loaded).
    pub async fn dispatch_on(
        &self,
        worker: Option<usize>,
        request: String,
        body: Option<Bytes>,
    ) -> Result<Reply, DispatchError> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let queued = Arc::new(AtomicUsize::new(0));
        let drained = Arc::new(Notify::new());
        let sink = Sink { tx, queued: queued.clone(), drained: drained.clone() };
        let worker = match worker {
            Some(i) => &self.workers[i % self.workers.len()],
            None => self.workers.iter().min_by_key(|w| w.inflight.load(Ordering::Relaxed)).expect("pool has workers"),
        };
        let id = self.next_id();
        if worker.tx.send(Job::Request { id, request, body, sink }).is_err() {
            return Err(DispatchError::NoResponse);
        }
        let mut guard = AbortGuard { tx: worker.tx.clone(), id, armed: true };
        let timeout = self.shared.limits.timeout;
        let head = match tokio::time::timeout(timeout, rx.recv()).await {
            Err(_) => return Err(DispatchError::Timeout(timeout)),
            Ok(None) => {
                guard.armed = false;
                return Err(DispatchError::NoResponse);
            }
            Ok(Some(Event::Head(h))) => h,
            Ok(Some(Event::Data(_))) => return Err(DispatchError::BadHead("body before head".into())),
        };
        let head: Head = serde_json::from_str(&head).map_err(|e| DispatchError::BadHead(e.to_string()))?;
        Ok(Reply { head, body: Body { rx, queued, drained, guard: Some(guard) } })
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// The worker that owns every socket for `key` (the URL path): same path, same worker,
    /// so module-level state in a socket route is one consistent room.
    pub fn socket_worker(&self, key: &str) -> usize {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        (h.finish() % self.workers.len() as u64) as usize
    }

    pub(crate) fn send_socket(&self, worker: usize, job: SocketJob) -> bool {
        self.workers[worker].tx.send(Job::Socket(job)).is_ok()
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
        assert_eq!(c.keys(""), ["a", "b"]);
        assert_eq!(c.keys("b"), ["b"]);
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
