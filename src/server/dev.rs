//! `forge dev`: build, serve, watch, rebuild, swap, and tell the browser to reload.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::Result;
use axum::{
    body::Body,
    http::{StatusCode, header},
    response::Response,
};
use notify::{EventKind, RecursiveMode, Watcher};

use super::{App, Server};
use crate::config::{Mode, Project, Target};

const IGNORED: &[&str] = &["node_modules", ".forge", "dist", ".git", "target"];

const LIVE_JS: &str = r#"let seen;
const es = new EventSource("/_forge/live");
const check = (e) => { if (seen !== undefined && seen !== e.data) location.reload(); seen = e.data; };
es.addEventListener("hello", check);
es.addEventListener("reload", check);
"#;

pub fn live_script() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(LIVE_JS))
        .unwrap()
}

pub fn live_events(server: &Server) -> Response {
    let Some(rx) = server.subscribe() else {
        return Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap();
    };
    let hello = format!("event: hello\ndata: {}\n\n", server.build_id());
    let state = (Some(hello), rx);
    let stream = futures_util::stream::unfold(state, |(hello, mut rx)| async move {
        if let Some(h) = hello {
            return Some((Ok::<_, std::io::Error>(bytes::Bytes::from(h)), (None, rx)));
        }
        let msg = match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Ok(id)) => format!("event: reload\ndata: {id}\n\n"),
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => ": lagged\n\n".to_string(),
            Ok(Err(_)) => return None,
            Err(_) => ": keep-alive\n\n".to_string(),
        };
        Some((Ok(bytes::Bytes::from(msg)), (None, rx)))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn rebuild(server: &Server, root: &Path) {
    let started = Instant::now();
    let project = match Project::load(root, Target::Server, Mode::Dev) {
        Ok(p) => p,
        Err(e) => return fail(server, e),
    };
    let result = match crate::build::build(&project).await {
        Ok(r) => r,
        Err(e) => return fail(server, e),
    };
    for w in &result.warnings {
        eprintln!("warn: {w}");
    }
    match App::new(project, result.manifest, result.server_code).await {
        Ok(app) => {
            let routes = app.manifest.routes.len();
            server.set_app(app);
            println!("✓ built {routes} routes in {} ms", started.elapsed().as_millis());
        }
        Err(e) => fail(server, e),
    }
}

fn fail(server: &Server, e: anyhow::Error) {
    let msg = format!("{e:#}");
    eprintln!("✗ {msg}");
    server.set_error(msg);
}

fn watch_roots(root: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    let mut out = vec![(root.to_path_buf(), RecursiveMode::NonRecursive)];
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() && !IGNORED.contains(&name.as_str()) && !name.starts_with('.') {
                out.push((e.path(), RecursiveMode::Recursive));
            }
        }
    }
    out
}

pub async fn run(root: PathBuf, addr: SocketAddr) -> Result<()> {
    let root = root.canonicalize()?;
    let server = Server::new(true);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("forge dev → http://{}", listener.local_addr()?);
    rebuild(&server, &root).await;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
            return;
        }
        let relevant = event
            .paths
            .iter()
            .any(|p| !p.components().any(|c| IGNORED.contains(&c.as_os_str().to_string_lossy().as_ref())));
        if relevant {
            let _ = tx.try_send(());
        }
    })?;
    for (path, mode) in watch_roots(&root) {
        let _ = watcher.watch(&path, mode);
    }

    let s = server.clone();
    tokio::spawn(async move {
        let _watcher = watcher;
        while rx.recv().await.is_some() {
            // Let editors finish their write-rename dance, then coalesce.
            tokio::time::sleep(Duration::from_millis(60)).await;
            while rx.try_recv().is_ok() {}
            rebuild(&s, &root).await;
        }
    });
    server.serve_on(listener).await
}
