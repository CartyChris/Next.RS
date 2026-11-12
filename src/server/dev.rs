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
/// Files directly in the project root that affect a build. Anything else there (logs,
/// notes, editor swap files) is ignored, so writing next to the project cannot loop.
const ROOT_FILES: &[&str] = &["forge.config.json", "package.json", "tsconfig.json", "jsconfig.json", ".env"];

/// Does a change to `path` require a rebuild?
fn relevant(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return false };
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    match parts.as_slice() {
        [] => false,
        [file] => ROOT_FILES.contains(&file.as_str()),
        [dir, ..] => {
            !IGNORED.contains(&dir.as_str())
                && !dir.starts_with('.')
                && !parts.iter().any(|p| p.ends_with('~') || p.ends_with(".swp"))
        }
    }
}

const LIVE_JS: &str = r#"let seen;
const es = new EventSource("/_forge/live");
const check = (e) => { if (seen !== undefined && seen !== e.data) location.reload(); seen = e.data; };
es.addEventListener("hello", check);
es.addEventListener("reload", check);

// Dev overlay: uncaught errors and rejected promises show up in the page.
const panel = document.createElement("forge-errors");
const shadow = panel.attachShadow({ mode: "open" });
shadow.innerHTML = `<style>
  :host { position: fixed; inset: auto 12px 12px 12px; z-index: 2147483647; font: 12px/1.45 ui-monospace, monospace; }
  .e { background: #1a0d0d; color: #ffd8d8; border: 1px solid #ff6b6b; border-radius: 8px; padding: 10px 12px; margin-top: 8px; box-shadow: 0 6px 24px #0008; max-height: 40vh; overflow: auto; }
  .t { color: #ff8787; font-weight: 700; display: flex; justify-content: space-between; gap: 12px; }
  pre { margin: 6px 0 0; white-space: pre-wrap; color: #f1f3f5; }
  button { all: unset; cursor: pointer; color: #adb5bd; }
</style><div id="list"></div>`;
const list = shadow.getElementById("list");
const show = (title, error) => {
  if (!panel.isConnected) document.documentElement.append(panel);
  const box = document.createElement("div");
  box.className = "e";
  const stack = (error && error.stack) || String(error ?? "");
  box.innerHTML = `<div class="t"><span></span><button title="dismiss">✕</button></div><pre></pre>`;
  box.querySelector("span").textContent = title;
  box.querySelector("pre").textContent = stack;
  box.querySelector("button").onclick = () => box.remove();
  list.append(box);
  while (list.children.length > 5) list.firstChild.remove();
};
addEventListener("error", (e) => show(/^Uncaught/.test(e.message) ? e.message : `Uncaught ${e.message}`, e.error ?? `${e.filename}:${e.lineno}`));
addEventListener("unhandledrejection", (e) => show(`Unhandled rejection: ${e.reason?.message ?? e.reason}`, e.reason));
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
    let watch_root = root.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
            return;
        }
        if event.paths.iter().any(|p| relevant(&watch_root, p)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_build_inputs_trigger_rebuilds() {
        let root = Path::new("/p");
        for yes in ["/p/app/page.tsx", "/p/public/x.png", "/p/forge.config.json", "/p/.env", "/p/lib/util.ts"] {
            assert!(relevant(root, Path::new(yes)), "{yes}");
        }
        for no in [
            "/p/dev.log",
            "/p/notes.md",
            "/p/.forge/x.js",
            "/p/node_modules/a/b.js",
            "/p/dist/index.html",
            "/p/.git/HEAD",
            "/p/app/.page.tsx.swp",
            "/other/app/x",
        ] {
            assert!(!relevant(root, Path::new(no)), "{no}");
        }
    }
}
