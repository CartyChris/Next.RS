//! Live development of a single HTML file: `forge dev --target single-html` and
//! `forge pack --watch`. Each change rebuilds the real artifact, the page at `/` is that file
//! (plus the live-reload script), and the browser reloads when it changes. Build errors show
//! in the page.

use std::{
    future::Future,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Result;
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{StatusCode, header},
    response::Response,
};
use notify::RecursiveMode;
use tokio::sync::broadcast;

use super::dev;
use crate::{build::html, render};

#[derive(Clone)]
pub struct SingleServer {
    inner: Arc<Inner>,
}

struct Inner {
    /// The last build: the file, or the error.
    page: Mutex<Result<Arc<str>, Arc<str>>>,
    id: Mutex<String>,
    live: broadcast::Sender<String>,
}

const LIVE_TAG: &str = r#"<script type="module" src="/_forge/live.js"></script>"#;

/// The file with the live-reload script, just before `</body>` (or at the end).
fn with_live_reload(page: &str) -> String {
    match page.to_ascii_lowercase().rfind("</body>") {
        Some(i) => format!("{}{LIVE_TAG}{}", &page[..i], &page[i..]),
        None => format!("{page}{LIVE_TAG}"),
    }
}

impl Default for SingleServer {
    fn default() -> Self {
        Self::new()
    }
}

impl SingleServer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                page: Mutex::new(Err("starting".into())),
                id: Mutex::new(String::new()),
                live: broadcast::channel(16).0,
            }),
        }
    }

    /// Publish a build result; open pages reload if it changed.
    pub fn set(&self, page: Result<String, String>) {
        let id = match &page {
            Ok(p) => crate::build::bundle::content_hash(p.as_bytes()),
            Err(e) => format!("error-{}", crate::build::bundle::content_hash(e.as_bytes())),
        };
        *self.inner.page.lock().unwrap() = page.map(Into::into).map_err(Into::into);
        let mut current = self.inner.id.lock().unwrap();
        if *current != id {
            *current = id.clone();
            let _ = self.inner.live.send(id);
        }
    }

    pub fn router(&self) -> Router {
        Router::new().fallback(handle).with_state(self.clone())
    }
}

async fn handle(State(server): State<SingleServer>, req: Request) -> Response {
    let path = req.uri().path();
    match path {
        "/_forge/live" => {
            let current = server.inner.id.lock().unwrap().clone();
            dev::live_stream(server.inner.live.subscribe(), current)
        }
        "/_forge/live.js" => dev::live_script(),
        // Single files route with `#/…`: every page is `/`.
        "/" | "/index.html" => {
            let page = server.inner.page.lock().unwrap().clone();
            let (status, body) = match page {
                Ok(p) => (StatusCode::OK, with_live_reload(&p)),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    render::simple_page("Build failed", &format!("<pre>{}</pre>{LIVE_TAG}", html::escape_html(&e))),
                ),
            };
            Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Body::from(body))
                .unwrap()
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("a single-file app has only `/` (it routes with #/…)"))
            .unwrap(),
    }
}

/// Build now, then on every change under `roots` that `filter` accepts; serve on `listener`.
pub async fn run<B, F>(
    listener: tokio::net::TcpListener,
    roots: Vec<(PathBuf, RecursiveMode)>,
    filter: impl Fn(&Path) -> bool + Send + 'static,
    build: B,
) -> Result<()>
where
    B: Fn() -> F + Send + Sync + 'static,
    F: Future<Output = Result<String>> + Send,
{
    let server = SingleServer::new();
    let rebuild = {
        let server = server.clone();
        move || {
            let server = server.clone();
            let started = Instant::now();
            let fut = build();
            async move {
                match fut.await {
                    Ok(page) => {
                        println!("✓ rebuilt ({} kB) in {} ms", page.len() / 1024, started.elapsed().as_millis());
                        server.set(Ok(page));
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        eprintln!("✗ {msg}");
                        server.set(Err(msg));
                    }
                }
            }
        }
    };
    rebuild().await;
    let (watcher, mut rx) = dev::watch(roots, filter)?;
    tokio::spawn(async move {
        let _watcher = watcher;
        while dev::next_change(&mut rx).await {
            rebuild().await;
        }
    });
    axum::serve(listener, server.router()).await?;
    Ok(())
}

/// `forge dev --target single-html`: the project's single-file build, uncompressed for
/// fast rebuilds and readable code.
pub async fn run_project(root: PathBuf, addr: SocketAddr) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("forge dev (single-html) → http://{}", listener.local_addr()?);
    run_project_on(root, listener).await
}

pub async fn run_project_on(root: PathBuf, listener: tokio::net::TcpListener) -> Result<()> {
    use crate::config::{Compress, Mode, Project, Target};
    let root = root.canonicalize()?;
    let watch_root = root.clone();
    let build_root = root.clone();
    run(
        listener,
        dev::watch_roots(&root),
        move |p| dev::relevant(&watch_root, p),
        move || {
            let root = build_root.clone();
            async move {
                let mut project = Project::load(&root, Target::SingleHtml, Mode::Dev)?;
                project.config.build.compress = Compress::Off;
                let result = crate::build::build(&project).await?;
                for w in &result.warnings {
                    eprintln!("warn: {w}");
                }
                let path = &result.artifacts.first().ok_or_else(|| anyhow::anyhow!("no single file was written"))?.0;
                Ok(std::fs::read_to_string(path)?)
            }
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_reload_goes_before_the_end_of_body() {
        assert_eq!(
            with_live_reload("<html><body><p>x</p></BODY></html>"),
            format!("<html><body><p>x</p>{LIVE_TAG}</BODY></html>")
        );
        assert_eq!(with_live_reload("<p>x</p>"), format!("<p>x</p>{LIVE_TAG}"));
    }
}
