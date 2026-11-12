//! The HTTP front end. One fallback handler routes everything; there is no middleware stack.
//!
//! ```text
//! /health/live, /health/ready     probes
//! /_forge/static/*                hashed client assets (immutable, precompressed)
//! /_forge/data?path=/x            loader data for client navigation (JSON, ETag)
//! /_forge/live, /_forge/live.js   dev live reload
//! /<file with extension>          public/ files
//! page routes                     SSR (streamed) with optional page cache
//! api routes                      any method, streamed responses
//! ```

pub mod dev;
mod files;
mod socket;

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header, request::Parts},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use bytes::Bytes;
use tokio::sync::broadcast;

use crate::{
    build::{
        html,
        manifest::{Manifest, RouteEntry},
    },
    config::Project,
    render::{self, Rendered},
    routes::{self, RouteKind},
    runtime::{DispatchError, Limits, Pool, Shared},
};

/// A loaded build: manifest + JS workers + page cache.
pub struct App {
    pub project: Project,
    pub manifest: Manifest,
    pub pool: Pool,
    pages: Arc<PageCache>,
}

impl App {
    pub async fn new(project: Project, manifest: Manifest, server_code: Arc<str>) -> Result<Self> {
        let s = &project.config.server;
        let limits = Limits {
            memory_bytes: s.memory_limit_mb << 20,
            cpu: Duration::from_millis(s.cpu_ms),
            timeout: Duration::from_millis(s.timeout_ms),
        };
        let shared = Arc::new(Shared::new(
            server_code,
            limits,
            project.server_env(),
            s.allow_fetch.clone(),
            project.mode.is_dev(),
        ));
        let pages = Arc::new(PageCache::new(s.page_cache_entries));
        let hook_pages = pages.clone();
        *shared.on_invalidate.lock().unwrap() = Some(Box::new(move |key: &str| hook_pages.invalidate(key)));
        let pool = Pool::start(shared, s.worker_count()).await?;
        Ok(Self { project, manifest, pool, pages })
    }

    /// Load a production build from `.forge/`.
    pub async fn load(project: Project) -> Result<Self> {
        let out = project.out_dir();
        let manifest = Manifest::load(&out)?;
        if manifest.target != crate::config::Target::Server {
            anyhow::bail!(
                "{} holds a `{}` build; `forge start` serves `server` builds",
                out.display(),
                manifest.target
            );
        }
        let capsule: crate::build::manifest::Capsule =
            serde_json::from_slice(&std::fs::read(out.join("capsule.json")).context("capsule.json missing")?)?;
        if capsule.build_id != manifest.build_id {
            anyhow::bail!("capsule.json and manifest.json are from different builds");
        }
        capsule.verify(&out)?;
        let code = std::fs::read_to_string(out.join(&manifest.server_entry))?;
        Self::new(project, manifest, code.into()).await
    }
}

/// Rendered pages for routes that export `revalidate`, served stale-while-revalidate.
struct PageCache {
    max: usize,
    entries: Mutex<HashMap<String, CachedPage>>,
    refreshing: Mutex<std::collections::HashSet<String>>,
}

#[derive(Clone)]
struct CachedPage {
    status: u16,
    body: Bytes,
    csp: Option<String>,
    stored: Instant,
    ttl: Duration,
}

impl PageCache {
    fn new(max: usize) -> Self {
        Self { max, entries: Mutex::new(HashMap::new()), refreshing: Mutex::new(Default::default()) }
    }
    fn get(&self, key: &str) -> Option<CachedPage> {
        self.entries.lock().unwrap().get(key).cloned()
    }
    fn put(&self, key: String, page: CachedPage) {
        if self.max == 0 {
            return;
        }
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.max && !entries.contains_key(&key) {
            if let Some(oldest) = entries.iter().min_by_key(|(_, p)| p.stored).map(|(k, _)| k.clone()) {
                entries.remove(&oldest);
            }
        }
        entries.insert(key, page);
    }
    /// `"/blog/x"` drops that path (any query), `"/blog/*"` a subtree, `"*"` everything.
    fn invalidate(&self, key: &str) {
        let mut entries = self.entries.lock().unwrap();
        if key == "*" {
            entries.clear();
        } else if let Some(prefix) = key.strip_suffix("/*") {
            entries.retain(|k, _| !(k.starts_with(prefix) && k[prefix.len()..].starts_with(['/', '?'])) && k != prefix);
        } else if key.starts_with('/') {
            entries.retain(|k, _| k.split('?').next() != Some(key));
        }
    }
}

enum Current {
    Starting,
    Ready(Arc<App>),
    Failed(Arc<str>),
}

#[derive(Clone)]
pub struct Server {
    inner: Arc<Inner>,
}

struct Inner {
    current: RwLock<Current>,
    /// Dev only: build ids for live reload.
    live: Option<broadcast::Sender<String>>,
    build_id: Mutex<String>,
}

impl Server {
    pub fn new(dev: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                current: RwLock::new(Current::Starting),
                live: dev.then(|| broadcast::channel(16).0),
                build_id: Mutex::new(String::new()),
            }),
        }
    }

    pub fn set_app(&self, app: App) {
        let id = app.manifest.build_id.clone();
        let old = std::mem::replace(&mut *self.inner.current.write().unwrap(), Current::Ready(Arc::new(app)));
        if let Current::Ready(old) = old {
            // Sockets hold the old code; clients reconnect to the new build.
            old.pool.shared.sockets.close_all(None, 1012, "server reloaded");
        }
        self.announce(id);
    }

    fn close_sockets(&self) {
        if let Current::Ready(app) = &*self.inner.current.read().unwrap() {
            app.pool.shared.sockets.close_all(None, 1001, "server shutting down");
        }
    }

    pub fn set_error(&self, message: String) {
        let id = format!("error-{}", crate::build::bundle::content_hash(message.as_bytes()));
        *self.inner.current.write().unwrap() = Current::Failed(message.into());
        self.announce(id);
    }

    pub fn build_id(&self) -> String {
        self.inner.build_id.lock().unwrap().clone()
    }

    pub(crate) fn subscribe(&self) -> Option<broadcast::Receiver<String>> {
        self.inner.live.as_ref().map(|tx| tx.subscribe())
    }

    fn announce(&self, id: String) {
        *self.inner.build_id.lock().unwrap() = id.clone();
        if let Some(tx) = &self.inner.live {
            let _ = tx.send(id);
        }
    }

    pub fn router(&self) -> Router {
        Router::new().fallback(handle).with_state(self.clone())
    }

    pub async fn serve(&self, addr: SocketAddr) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("binding {addr}"))?;
        self.serve_on(listener).await
    }

    pub async fn serve_on(&self, listener: tokio::net::TcpListener) -> Result<()> {
        let me = self.clone();
        let shutdown = async move {
            shutdown_signal().await;
            me.close_sockets();
        };
        axum::serve(listener, self.router()).with_graceful_shutdown(shutdown).await?;
        Ok(())
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}

async fn handle(State(server): State<Server>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let dev = server.inner.live.is_some();
    match path.as_str() {
        "/health/live" => return text(StatusCode::OK, "ok"),
        "/_forge/live" if dev => return dev::live_events(&server),
        "/_forge/live.js" if dev => return dev::live_script(),
        _ => {}
    }
    let app = match &*server.inner.current.read().unwrap() {
        Current::Ready(app) => app.clone(),
        Current::Starting => return text(StatusCode::SERVICE_UNAVAILABLE, "starting"),
        Current::Failed(msg) => {
            if path == "/health/ready" {
                return text(StatusCode::SERVICE_UNAVAILABLE, "build failed");
            }
            let body = render::simple_page(
                "Build failed",
                &format!(
                    "<pre>{}</pre><script type=\"module\" src=\"{}\"></script>",
                    html::escape_html(msg),
                    html::LIVE_RELOAD_PATH
                ),
            );
            return html_response(StatusCode::INTERNAL_SERVER_ERROR, body.into(), None);
        }
    };
    if path == "/health/ready" {
        let body = serde_json::json!({
            "ok": true,
            "buildId": app.manifest.build_id,
            "workers": app.pool.len(),
            "inflight": app.pool.inflight(),
            "sockets": app.pool.shared.sockets.count(),
        });
        return json_response(StatusCode::OK, body.to_string().into(), None);
    }
    if let Some(rest) = path.strip_prefix(crate::build::bundle::STATIC_PREFIX) {
        return files::serve(&app.project.out_dir().join("client"), rest, req.headers(), !dev).await;
    }
    let method = req.method().clone();
    let last = path.rsplit('/').next().unwrap_or_default();
    if last.contains('.') && matches!(method, Method::GET | Method::HEAD) {
        if let Some(resp) = files::serve_public(&app.project.public_dir(), &path, req.headers()).await {
            return resp;
        }
    }
    if path == "/_forge/data" {
        return data(&app, req.into_parts().0).await;
    }
    let Some(parts) = routes::split_path(&path) else {
        return text(StatusCode::BAD_REQUEST, "bad path");
    };
    for route in &app.manifest.routes {
        if let Some(params) = routes::match_segments(&route.segments, &parts) {
            return match route.kind {
                RouteKind::Api if route.socket && socket::is_upgrade(req.headers()) => {
                    socket::upgrade(app.clone(), route, params, req).await
                }
                RouteKind::Api => api(&app, route, params, req).await,
                RouteKind::Page if matches!(method, Method::GET | Method::HEAD) => {
                    page(&app, route, params, req.into_parts().0, dev).await
                }
                RouteKind::Page => {
                    let mut r = text(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
                    r.headers_mut().insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
                    r
                }
            };
        }
    }
    not_found(&app, req.into_parts().0, dev).await
}

pub(crate) fn request_url(uri: &Uri, headers: &HeaderMap) -> String {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost");
    let proto = headers.get("x-forwarded-proto").and_then(|h| h.to_str().ok()).unwrap_or("http");
    let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    format!("{proto}://{host}{pq}")
}

pub(crate) fn header_list(headers: &HeaderMap) -> Vec<(String, String)> {
    headers.iter().filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string()))).collect()
}

fn nonce() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::fill(&mut b);
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn is_bot(headers: &HeaderMap) -> bool {
    let ua = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    ["bot", "crawl", "spider", "slurp", "facebookexternalhit", "embedly", "preview", "lighthouse"]
        .iter()
        .any(|s| ua.contains(s))
}

fn secure(resp: &mut Response) {
    let h = resp.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin"));
}

fn text(status: StatusCode, body: &'static str) -> Response {
    let mut r = (status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response();
    secure(&mut r);
    r
}

fn json_response(status: StatusCode, body: Bytes, etag: Option<&str>) -> Response {
    let mut r = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .unwrap();
    if let Some(e) = etag.and_then(|e| HeaderValue::from_str(e).ok()) {
        r.headers_mut().insert(header::ETAG, e);
    }
    secure(&mut r);
    r
}

fn html_response(status: StatusCode, body: Body, csp: Option<&str>) -> Response {
    let mut r =
        Response::builder().status(status).header(header::CONTENT_TYPE, "text/html; charset=utf-8").body(body).unwrap();
    if let Some(v) = csp.and_then(|c| HeaderValue::from_str(c).ok()) {
        r.headers_mut().insert(header::CONTENT_SECURITY_POLICY, v);
    }
    secure(&mut r);
    r
}

fn dispatch_error(e: DispatchError, dev: bool) -> Response {
    let status = match e {
        DispatchError::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    eprintln!("[forge] {e}");
    let detail = if dev { format!("<pre>{}</pre>", html::escape_html(&e.to_string())) } else { String::new() };
    html_response(status, render::simple_page(status.canonical_reason().unwrap_or("Error"), &detail).into(), None)
}

fn page_body(open: String, body: crate::runtime::Body) -> Body {
    use futures_util::StreamExt;
    let head = futures_util::stream::once(async move { Ok::<_, std::io::Error>(Bytes::from(open)) });
    let tail = futures_util::stream::once(async { Ok(Bytes::from_static(html::Document::close().as_bytes())) });
    Body::from_stream(head.chain(body.into_stream()).chain(tail))
}

enum PageOut {
    Html { status: StatusCode, body: Body, csp: Option<String> },
    Raw { status: StatusCode, body: Body, headers: Vec<(String, String)> },
}

async fn render_page(
    app: &App,
    route_id: &str,
    params: &routes::Params,
    uri: &Uri,
    headers: &HeaderMap,
    stream: bool,
    dev: bool,
) -> Result<PageOut, Response> {
    let nonce = nonce();
    let url = request_url(uri, headers);
    let headers = header_list(headers);
    let request = render::Request {
        kind: "page",
        route: route_id,
        params: render::params_json(params),
        pathname: uri.path(),
        url: &url,
        method: "GET",
        headers: &headers,
        nonce: &nonce,
        stream,
    };
    let opts = render::PageOptions { nonce: Some(&nonce), live_reload: dev };
    match render::page(&app.pool, &app.manifest, &request, &opts).await {
        Ok(Rendered::Page { status, open, body }) => Ok(PageOut::Html {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
            body: page_body(open, body),
            csp: html::csp(&app.project.config.security, &nonce, dev),
        }),
        Ok(Rendered::Raw { builtin_not_found: true, .. }) => Err(builtin_not_found()),
        Ok(Rendered::Raw { status, headers, body, .. }) => Ok(PageOut::Raw {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
            body: Body::from_stream(body.into_stream()),
            headers,
        }),
        Err(e) => Err(dispatch_error(e, dev)),
    }
}

async fn page(app: &Arc<App>, route: &RouteEntry, params: routes::Params, req: Parts, dev: bool) -> Response {
    let key = req.uri.path_and_query().map(|p| p.to_string()).unwrap_or_default();
    let cacheable = route.revalidate.is_some() && !dev;
    if cacheable {
        if let Some(hit) = app.pages.get(&key) {
            if hit.stored.elapsed() > hit.ttl {
                refresh_in_background(
                    app.clone(),
                    route.clone(),
                    params.clone(),
                    req.headers.clone(),
                    req.uri.clone(),
                    key.clone(),
                );
            }
            let mut r = html_response(
                StatusCode::from_u16(hit.status).unwrap_or(StatusCode::OK),
                Body::from(hit.body),
                hit.csp.as_deref(),
            );
            r.headers_mut().insert("x-forge-cache", HeaderValue::from_static("HIT"));
            return r;
        }
    }
    let stream = route.streaming && !cacheable && !is_bot(&req.headers);
    match render_page(app, &route.id, &params, &req.uri, &req.headers, stream, dev).await {
        Err(resp) => resp,
        Ok(PageOut::Raw { status, body, headers }) => raw_response(status, body, headers),
        Ok(PageOut::Html { status, body, csp }) => {
            if cacheable && status == StatusCode::OK {
                let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
                    return text(StatusCode::INTERNAL_SERVER_ERROR, "render failed");
                };
                let ttl = Duration::from_secs_f64(route.revalidate.unwrap_or(0.0));
                app.pages.put(
                    key,
                    CachedPage { status: 200, body: bytes.clone(), csp: csp.clone(), stored: Instant::now(), ttl },
                );
                let mut r = html_response(status, Body::from(bytes), csp.as_deref());
                r.headers_mut().insert("x-forge-cache", HeaderValue::from_static("MISS"));
                return r;
            }
            html_response(status, body, csp.as_deref())
        }
    }
}

fn refresh_in_background(
    app: Arc<App>,
    route: RouteEntry,
    params: routes::Params,
    headers: HeaderMap,
    uri: Uri,
    key: String,
) {
    if !app.pages.refreshing.lock().unwrap().insert(key.clone()) {
        return;
    }
    tokio::spawn(async move {
        if let Ok(PageOut::Html { status: StatusCode::OK, body, csp }) =
            render_page(&app, &route.id, &params, &uri, &headers, false, false).await
        {
            if let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await {
                let ttl = Duration::from_secs_f64(route.revalidate.unwrap_or(0.0));
                app.pages.put(key.clone(), CachedPage { status: 200, body: bytes, csp, stored: Instant::now(), ttl });
            }
        }
        app.pages.refreshing.lock().unwrap().remove(&key);
    });
}

fn raw_response(status: StatusCode, body: Body, headers: Vec<(String, String)>) -> Response {
    let mut r = Response::builder().status(status).body(body).unwrap();
    for (k, v) in headers {
        let lower = k.to_ascii_lowercase();
        if matches!(lower.as_str(), "content-length" | "transfer-encoding" | "connection" | "keep-alive") {
            continue;
        }
        if let (Ok(k), Ok(v)) = (HeaderName::from_bytes(lower.as_bytes()), HeaderValue::from_str(&v)) {
            r.headers_mut().append(k, v);
        }
    }
    secure(&mut r);
    r
}

fn builtin_not_found() -> Response {
    html_response(
        StatusCode::NOT_FOUND,
        render::simple_page("404", "<p>This page could not be found.</p>").into(),
        None,
    )
}

async fn not_found(app: &Arc<App>, req: Parts, dev: bool) -> Response {
    if app.manifest.not_found.is_none() || !matches!(req.method, Method::GET | Method::HEAD) {
        return builtin_not_found();
    }
    match render_page(app, crate::build::generated::NOT_FOUND_ID, &Vec::new(), &req.uri, &req.headers, false, dev).await
    {
        Ok(PageOut::Html { body, csp, .. }) => html_response(StatusCode::NOT_FOUND, body, csp.as_deref()),
        Ok(PageOut::Raw { status, body, headers }) => raw_response(status, body, headers),
        Err(resp) => resp,
    }
}

async fn data(app: &Arc<App>, req: Parts) -> Response {
    let query = req.uri.query().unwrap_or_default();
    let Some(target) =
        url::form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "path").map(|(_, v)| v.into_owned())
    else {
        return text(StatusCode::BAD_REQUEST, "missing ?path=");
    };
    let (pathname, search) = target.split_once('?').map_or((target.as_str(), ""), |(p, q)| (p, q));
    let Some(parts) = routes::split_path(pathname).filter(|_| pathname.starts_with('/')) else {
        return text(StatusCode::BAD_REQUEST, "bad path");
    };
    let matched = app
        .manifest
        .routes
        .iter()
        .filter(|r| r.kind == RouteKind::Page)
        .find_map(|r| routes::match_segments(&r.segments, &parts).map(|p| (r, p)));
    let Some((route, params)) = matched else {
        return json_response(StatusCode::NOT_FOUND, Bytes::from_static(br#"{"notFound":true}"#), None);
    };
    if !route.loader {
        return json_response(StatusCode::OK, Bytes::from_static(br#"{"data":null}"#), None);
    }
    let host = req.headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost");
    let url = format!("http://{host}{pathname}{}{search}", if search.is_empty() { "" } else { "?" });
    let headers = header_list(&req.headers);
    let request = render::Request {
        kind: "data",
        route: &route.id,
        params: render::params_json(&params),
        pathname,
        url: &url,
        method: "GET",
        headers: &headers,
        nonce: "",
        stream: false,
    };
    let dev = app.project.mode.is_dev();
    let reply = match app.pool.dispatch(serde_json::to_string(&request).unwrap(), None).await {
        Ok(r) => r,
        Err(e) => return dispatch_error(e, dev),
    };
    let status = StatusCode::from_u16(reply.head.status).unwrap_or(StatusCode::OK);
    let body = Bytes::from(reply.body.collect().await);
    if !status.is_success() {
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(body))
            .unwrap();
    }
    let etag = format!("\"{}\"", crate::build::bundle::content_hash(&body));
    if req.headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .body(Body::empty())
            .unwrap();
    }
    json_response(StatusCode::OK, body, Some(&etag))
}

async fn api(app: &Arc<App>, route: &RouteEntry, params: routes::Params, req: Request) -> Response {
    let dev = app.project.mode.is_dev();
    let (parts, body) = req.into_parts();
    let url = request_url(&parts.uri, &parts.headers);
    let method = parts.method.as_str().to_string();
    let headers = header_list(&parts.headers);
    let pathname = parts.uri.path().to_string();
    let limit = app.project.config.server.max_body_bytes;
    let body = match axum::body::to_bytes(body, limit).await {
        Ok(b) if b.is_empty() => None,
        Ok(b) => Some(b),
        Err(_) => return text(StatusCode::PAYLOAD_TOO_LARGE, "request body too large"),
    };
    let request = render::Request {
        kind: "api",
        route: &route.id,
        params: render::params_json(&params),
        pathname: &pathname,
        url: &url,
        method: &method,
        headers: &headers,
        nonce: "",
        stream: false,
    };
    match app.pool.dispatch(serde_json::to_string(&request).unwrap(), body).await {
        Ok(reply) => raw_response(
            StatusCode::from_u16(reply.head.status).unwrap_or(StatusCode::OK),
            Body::from_stream(reply.body.into_stream()),
            reply.head.headers,
        ),
        Err(e) => dispatch_error(e, dev),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(body: &str) -> CachedPage {
        CachedPage {
            status: 200,
            body: Bytes::copy_from_slice(body.as_bytes()),
            csp: None,
            stored: Instant::now(),
            ttl: Duration::from_secs(60),
        }
    }

    #[test]
    fn page_cache_invalidation() {
        let c = PageCache::new(10);
        for k in ["/", "/blog", "/blog/a", "/blog/a?x=1", "/blogger"] {
            c.put(k.into(), page(k));
        }
        c.invalidate("/blog/a");
        assert!(c.get("/blog/a").is_none() && c.get("/blog/a?x=1").is_none() && c.get("/blog").is_some());
        c.invalidate("/blog/*");
        assert!(c.get("/blog").is_none() && c.get("/blogger").is_some() && c.get("/").is_some());
        c.invalidate("*");
        assert!(c.get("/").is_none());
    }

    #[test]
    fn page_cache_evicts_oldest() {
        let c = PageCache::new(2);
        c.put("/a".into(), page("a"));
        std::thread::sleep(Duration::from_millis(2));
        c.put("/b".into(), page("b"));
        c.put("/c".into(), page("c"));
        assert!(c.get("/a").is_none() && c.get("/b").is_some() && c.get("/c").is_some());
    }
}
