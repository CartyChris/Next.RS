#![allow(dead_code)]

use std::path::{Path, PathBuf};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode},
};
use nextrs::{
    build::{self, BuildResult},
    config::{Mode, Project, Target},
    server::{App, Server},
};
use tower::ServiceExt;

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn temp_dir(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("forge-it-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A project from inline files.
pub fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = temp_dir(name);
    for (rel, contents) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    dir
}

/// A copy of `examples/<name>` (its node_modules linked, not copied).
pub fn example(name: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join(name);
    let dir = temp_dir(name);
    copy(&src, &dir);
    let modules = src.join("node_modules");
    if modules.is_dir() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&modules, dir.join("node_modules")).unwrap();
    }
    dir
}

fn copy(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if [".forge", "dist", "node_modules"].contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        let target = to.join(&name);
        if entry.path().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub async fn build(root: &Path, target: Target) -> anyhow::Result<BuildResult> {
    let project = Project::load(root, target, Mode::Prod)?;
    build::build(&project).await
}

/// Build for the server target and return an in-process router.
pub async fn serve(root: &Path) -> Router {
    let result = build(root, Target::Server).await.unwrap_or_else(|e| panic!("build failed: {e:#}"));
    let project = Project::load(root, Target::Server, Mode::Prod).unwrap();
    let app = App::new(project, result.manifest, result.server_code).await.unwrap();
    let server = Server::new(false);
    server.set_app(app);
    server.router()
}

pub struct Res {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

pub async fn request(router: &Router, method: Method, uri: &str, headers: &[(&str, &str)], body: &str) -> Res {
    let mut req = Request::builder().method(method).uri(uri).header("host", "localhost");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = router.clone().oneshot(req.body(Body::from(body.to_string())).unwrap()).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    Res { status, headers, body: String::from_utf8_lossy(&bytes).into_owned() }
}

pub async fn get(router: &Router, uri: &str) -> Res {
    request(router, Method::GET, uri, &[], "").await
}
