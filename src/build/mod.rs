//! `forge build`: scan → analyze → generate → bundle (server, client) → per-target output.
//!
//! | target        | output                                   | runtime                         |
//! |---------------|------------------------------------------|---------------------------------|
//! | `server`      | `.forge/{server,client}/` + manifest     | `forge start` (SSR, data, APIs) |
//! | `static`      | `dist/` prerendered HTML + data JSON     | any static host                 |
//! | `single-html` | `dist/index.html`, everything inlined    | a browser, even from `file://`  |

pub mod analyze;
pub mod bundle;
pub mod generated;
pub mod html;
pub mod manifest;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::Value;

use self::{
    analyze::{Analysis, Literal, Policy, analyze},
    bundle::{Bundle, BundleSpec, Side},
    generated::{EmitInput, NOT_FOUND_ID, PageInfo, PageKind, VIRTUAL_MODULES},
    manifest::{BundleStats, Capabilities, Capsule, ClientAssets, Manifest, RouteEntry, SCHEMA},
};
use crate::{
    config::{Project, Target},
    render,
    routes::{self, RouteKind, RouteTable},
    runtime::{Limits, Pool, Shared},
};

pub const FORGE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const TYPES_DTS: &str = include_str!("../runtime/js/forge.d.ts");
pub const HTTP_METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];

#[derive(Debug)]
pub struct BuildResult {
    pub manifest: Manifest,
    pub server_code: Arc<str>,
    pub stats: BundleStats,
    pub warnings: Vec<String>,
    /// Files a person ships (static and single-html targets): `(path, bytes)`.
    pub artifacts: Vec<(PathBuf, u64)>,
    pub elapsed: Duration,
}

/// The analyzed route table: everything known before bundling.
#[derive(Debug)]
pub struct Plan {
    pub table: RouteTable,
    pub pages: BTreeMap<String, PageInfo>,
    pub routes: Vec<RouteEntry>,
    pub not_found: Option<RouteEntry>,
}

/// Parses each source file once.
struct Analyzer<'a> {
    root: &'a Path,
    policy: Policy,
    cache: HashMap<String, Analysis>,
}

impl Analyzer<'_> {
    fn get(&mut self, file: &str) -> Result<&Analysis> {
        if !self.cache.contains_key(file) {
            let src = std::fs::read_to_string(self.root.join(file)).with_context(|| format!("reading {file}"))?;
            let analysis = analyze(file, &src, self.policy)?;
            self.cache.insert(file.to_string(), analysis);
        }
        Ok(&self.cache[file])
    }

    fn page(
        &mut self,
        id: &str,
        pattern: &str,
        segments: &[routes::Segment],
        file: &str,
        layouts: &[String],
        loader: Option<&String>,
    ) -> Result<(RouteEntry, PageInfo)> {
        for layout in layouts {
            if !self.get(layout)?.has("default") {
                bail!("{layout}: a layout must default-export a React component");
            }
        }
        if let Some(l) = loader {
            let a = self.get(l)?;
            if !a.has("loader") && !a.has("staticParams") {
                bail!("{l}: a loader file must export `loader(ctx)` (and optionally `staticParams()`)");
            }
        }
        let a = self.get(file)?;
        let ssr_off = matches!(a.literals.get("ssr"), Some(Literal::Bool(false)));
        let info = if a.has("default") {
            PageInfo { kind: PageKind::React, ssr: !ssr_off }
        } else if a.has("mount") || a.has("render") {
            // Inside React layouts a vanilla page is hosted by the React adapter.
            let kind = if layouts.is_empty() { PageKind::Vanilla } else { PageKind::React };
            PageInfo { kind, ssr: a.has("render") && !ssr_off }
        } else {
            bail!(
                "{file}: a page must default-export a React component, or export `mount(el, ctx)` and/or `render(ctx)`"
            );
        };
        let entry = RouteEntry {
            id: id.to_string(),
            pattern: pattern.to_string(),
            segments: segments.to_vec(),
            kind: RouteKind::Page,
            file: file.to_string(),
            render: Some(info.kind),
            ssr: info.ssr,
            revalidate: a.number("revalidate").filter(|n| *n > 0.0),
            streaming: !matches!(a.literals.get("streaming"), Some(Literal::Bool(false))),
            loader: loader.is_some(),
            socket: false,
        };
        Ok((entry, info))
    }
}

pub fn plan(project: &Project) -> Result<Plan> {
    let table = routes::scan(&project.root, &project.app_dir())?;
    if table.routes.is_empty() {
        bail!("no routes in {0}/: add {0}/page.tsx (or page.jsx / page.ts / page.js)", project.config.app_dir);
    }
    let mut an = Analyzer {
        root: &project.root,
        policy: Policy { allow_eval: project.config.security.allow_eval },
        cache: HashMap::new(),
    };
    let mut pages = BTreeMap::new();
    let mut entries = Vec::new();
    for r in &table.routes {
        match r.kind {
            RouteKind::Page => {
                let (entry, info) = an.page(&r.id, &r.pattern, &r.segments, &r.file, &r.layouts, r.loader.as_ref())?;
                pages.insert(r.id.clone(), info);
                entries.push(entry);
            }
            RouteKind::Api => {
                let a = an.get(&r.file)?;
                let socket = a.has("socket");
                if !socket && !HTTP_METHODS.iter().any(|m| a.has(m)) {
                    bail!(
                        "{}: an API route must export at least one of {}, or `socket`",
                        r.file,
                        HTTP_METHODS.join(", ")
                    );
                }
                entries.push(RouteEntry {
                    id: r.id.clone(),
                    pattern: r.pattern.clone(),
                    segments: r.segments.clone(),
                    kind: RouteKind::Api,
                    file: r.file.clone(),
                    render: None,
                    ssr: false,
                    revalidate: None,
                    streaming: false,
                    loader: false,
                    socket,
                });
            }
        }
    }
    let not_found = match &table.not_found {
        Some(file) => {
            let (entry, info) = an.page(NOT_FOUND_ID, "", &[], file, &table.root_layouts, None)?;
            pages.insert(NOT_FOUND_ID.to_string(), info);
            Some(entry)
        }
        None => None,
    };
    Ok(Plan { table, pages, routes: entries, not_found })
}

fn defines(project: &Project, server: bool) -> Vec<(String, String)> {
    let env = project.mode.node_env();
    let dev = project.mode.is_dev();
    let mut out = vec![
        ("process.env.NODE_ENV".to_string(), format!("\"{env}\"")),
        ("import.meta.env.MODE".to_string(), format!("\"{env}\"")),
        ("import.meta.env.DEV".to_string(), dev.to_string()),
        ("import.meta.env.PROD".to_string(), (!dev).to_string()),
        ("import.meta.env.SSR".to_string(), server.to_string()),
    ];
    if server {
        // The server bundle is evaluated without a module URL; give relative `new URL()`s a base.
        out.push(("import.meta.url".to_string(), "\"file:///forge/server.js\"".to_string()));
    }
    out.extend(project.config.build.define.iter().map(|(k, v)| (k.clone(), v.clone())));
    out
}

fn aliases(gen_dir: &Path, side: Side) -> Vec<(String, PathBuf)> {
    VIRTUAL_MODULES
        .iter()
        .map(|(spec, server, client)| {
            (spec.to_string(), gen_dir.join("runtime").join(if side == Side::Server { server } else { client }))
        })
        .collect()
}

fn spec(project: &Project, input: (&str, &Path), side: Side) -> BundleSpec {
    let gen_dir = project.generated_dir();
    let client = side == Side::Client;
    BundleSpec {
        root: project.root.clone(),
        inputs: vec![(input.0.to_string(), input.1.to_path_buf())],
        side,
        minify: client && project.minify(),
        sourcemap: client && project.sourcemap(),
        split: client && project.target != Target::SingleHtml,
        inline_assets: project.target == Target::SingleHtml,
        define: defines(project, !client),
        aliases: aliases(&gen_dir, side),
        // Both builds scan: client-only pages are never imported by the server bundle.
        scan: Some(Policy { allow_eval: project.config.security.allow_eval }),
        generated_dir: gen_dir,
        policy: Policy { allow_eval: project.config.security.allow_eval },
        depth: 0,
        classic: false,
    }
}

/// Routes, analysis and the security scan, without writing build output.
pub async fn check(project: &Project) -> Result<(Plan, Vec<String>)> {
    let plan = plan(project)?;
    let generated = generated::emit(&EmitInput {
        root: &project.root,
        dir: &project.generated_dir(),
        table: &plan.table,
        pages: &plan.pages,
        mode: project.mode,
        target: project.target,
        static_data: None,
    })?;
    let server = bundle::bundle(spec(project, ("server", &generated.server_entry), Side::Server)).await?;
    Ok((plan, server.warnings))
}

pub async fn build(project: &Project) -> Result<BuildResult> {
    let started = Instant::now();
    if project.target == Target::WasmHost {
        bail!("the wasm-host target is planned but not implemented yet (see docs/ROADMAP.md)");
    }
    let plan = plan(project)?;
    let out_dir = project.out_dir();
    let gen_dir = project.generated_dir();
    std::fs::create_dir_all(out_dir.join("types"))?;
    generated::write_if_changed(&out_dir.join("types/forge.d.ts"), TYPES_DTS)?;

    let emit = |static_data: Option<&str>| {
        generated::emit(&EmitInput {
            root: &project.root,
            dir: &gen_dir,
            table: &plan.table,
            pages: &plan.pages,
            mode: project.mode,
            target: project.target,
            static_data,
        })
    };
    let generated = emit(None)?;
    let server = bundle::bundle(spec(project, ("server", &generated.server_entry), Side::Server)).await?;
    let server_code: Arc<str> = server.entry("server").context("server bundle has no entry chunk")?.code.clone().into();
    let mut warnings = server.warnings.clone();
    if project.target != Target::Server && plan.routes.iter().any(|r| r.kind == RouteKind::Api) {
        warnings.push(format!(
            "API routes do not exist in the {} target; they were type-checked and scanned only",
            project.target
        ));
    }
    let build_id = bundle::content_hash(server_code.as_bytes());

    let mut result = match project.target {
        Target::Server => build_server(project, &plan, &generated, &server, server_code.clone(), &build_id).await?,
        Target::Static => build_static(project, &plan, &generated, &server, server_code.clone(), &build_id).await?,
        Target::SingleHtml => build_single(project, &plan, &server, server_code.clone(), &build_id, emit).await?,
        Target::WasmHost => unreachable!(),
    };
    result.warnings.splice(0..0, warnings);
    std::fs::write(out_dir.join("manifest.json"), serde_json::to_vec_pretty(&result.manifest)?)?;
    std::fs::write(out_dir.join("bundle-stats.json"), serde_json::to_vec_pretty(&result.stats)?)?;
    result.elapsed = started.elapsed();
    Ok(result)
}

fn manifest(project: &Project, plan: &Plan, build_id: &str, client: ClientAssets) -> Manifest {
    Manifest {
        schema: SCHEMA,
        forge_version: FORGE_VERSION.to_string(),
        build_id: build_id.to_string(),
        target: project.target,
        mode: project.mode,
        capabilities: Capabilities::for_target(project.target),
        lang: project.config.lang.clone(),
        title: project.config.title.clone(),
        routes: plan.routes.clone(),
        not_found: plan.not_found.clone(),
        client,
        server_entry: "server/server.js".to_string(),
    }
}

/// Write the client bundle into `dir` (wiped first) and describe it for the manifest.
/// Server-only assets (imported by a loader or API route) are served from the client directory too.
fn with_server_assets(mut client: Bundle, server: &Bundle) -> Bundle {
    for (name, bytes) in &server.files {
        if !client.files.iter().any(|(n, _)| n == name) {
            client.files.push((name.clone(), bytes.clone()));
        }
    }
    client
}

fn write_client(dir: &Path, client: &Bundle, generated: &generated::Generated) -> Result<(ClientAssets, Vec<String>)> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let mut files = Vec::new();
    let mut write = |rel: &str, bytes: &[u8]| -> Result<()> {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
        files.push(rel.to_string());
        Ok(())
    };
    for chunk in &client.chunks {
        write(&chunk.file, chunk.code.as_bytes())?;
        if let Some(map) = &chunk.map {
            write(&format!("{}.map", chunk.file), map.as_bytes())?;
        }
    }
    for (rel, bytes) in &client.files {
        write(rel, bytes)?;
    }
    let css = if client.css.is_empty() {
        None
    } else {
        let name = format!("styles.{}.css", bundle::content_hash(client.css.as_bytes()));
        write(&name, client.css.as_bytes())?;
        Some(render::static_url(&name))
    };

    let entry = client.entry("client").context("client bundle has no entry chunk")?;
    let entry_closure = client.static_closure(entry);
    let mut preload = BTreeMap::new();
    for (id, module) in &generated.route_modules {
        let module = module.to_string_lossy();
        let mut list: Vec<String> = entry_closure.iter().skip(1).cloned().collect();
        if let Some(chunk) = client.chunks.iter().find(|c| c.facade.as_deref() == Some(&*module)) {
            for f in client.static_closure(chunk) {
                if f != entry.file && !list.contains(&f) {
                    list.push(f);
                }
            }
        }
        preload.insert(id.clone(), list.iter().map(|f| render::static_url(f)).collect());
    }
    Ok((ClientAssets { entry: render::static_url(&entry.file), css, preload }, files))
}

async fn build_server(
    project: &Project,
    plan: &Plan,
    generated: &generated::Generated,
    server: &Bundle,
    server_code: Arc<str>,
    build_id: &str,
) -> Result<BuildResult> {
    let out_dir = project.out_dir();
    let client = with_server_assets(
        bundle::bundle(spec(project, ("client", &generated.client_entry), Side::Client)).await?,
        server,
    );
    let (assets, client_files) = write_client(&out_dir.join("client"), &client, generated)?;
    std::fs::create_dir_all(out_dir.join("server"))?;
    std::fs::write(out_dir.join("server/server.js"), server_code.as_bytes())?;

    let mut tracked: Vec<String> = client_files.iter().map(|f| format!("client/{f}")).collect();
    if !project.mode.is_dev() {
        let compressed = manifest::precompress(&out_dir.join("client"), &client_files)?;
        tracked.extend(compressed.into_iter().map(|f| format!("client/{f}")));
    }
    tracked.push("server/server.js".into());
    let capsule = Capsule::of(build_id, &out_dir, &tracked)?;
    std::fs::write(out_dir.join("capsule.json"), serde_json::to_vec_pretty(&capsule)?)?;

    Ok(BuildResult {
        manifest: manifest(project, plan, build_id, assets),
        server_code,
        stats: BundleStats::of(Some(&client), server, &project.root),
        warnings: client.warnings,
        artifacts: Vec::new(),
        elapsed: Duration::ZERO,
    })
}

/// A one-off pool for build-time rendering (prerender, loaders, staticParams).
pub async fn build_pool(project: &Project, code: Arc<str>, workers: usize) -> Result<Pool> {
    let s = &project.config.server;
    let limits = Limits {
        memory_bytes: s.memory_limit_mb << 20,
        cpu: Duration::from_millis(s.cpu_ms.max(10_000)),
        timeout: Duration::from_millis(s.timeout_ms.max(60_000)),
    };
    let shared = Shared::new(code, limits, project.server_env(), s.allow_fetch.clone(), project.mode.is_dev());
    Pool::start(Arc::new(shared), workers).await.context("loading the server bundle")
}

async fn call(
    pool: &Pool,
    kind: &str,
    route: &str,
    pathname: &str,
    params: &routes::Params,
) -> Result<(u16, Vec<u8>, Option<crate::runtime::PageHead>)> {
    let url = format!("http://localhost{pathname}");
    let req = render::Request {
        kind,
        route,
        params: render::params_json(params),
        pathname,
        url: &url,
        method: "GET",
        headers: &[],
        nonce: "",
        stream: false,
    };
    let reply = pool
        .dispatch(serde_json::to_string(&req)?, None)
        .await
        .map_err(|e| anyhow::anyhow!("{route} ({pathname}): {e}"))?;
    let status = reply.head.status;
    let page = reply.head.page;
    let body = reply.body.collect().await;
    if status >= 500 {
        bail!("{pathname}: server error\n{}", strip_tags(&String::from_utf8_lossy(&body)));
    }
    Ok((status, body, page))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

/// Every concrete pathname of a page route: itself if static, else what `staticParams()` lists.
async fn concrete_paths(pool: &Pool, route: &RouteEntry) -> Result<Vec<(String, routes::Params)>> {
    if route.segments.iter().all(|s| matches!(s, routes::Segment::Static(_))) {
        return Ok(vec![(routes::fill(&route.segments, &HashMap::new()).unwrap_or_else(|| "/".into()), Vec::new())]);
    }
    if !route.loader {
        return Ok(Vec::new());
    }
    let (_, body, _) = call(pool, "paths", &route.id, &route.pattern, &Vec::new()).await?;
    let list: Vec<serde_json::Map<String, Value>> = serde_json::from_slice(&body)
        .with_context(|| format!("{}: staticParams() must return an array of params objects", route.pattern))?;
    let mut out = Vec::new();
    for item in list {
        let mut params = HashMap::new();
        for (k, v) in item {
            let value = match v {
                Value::String(s) => s,
                Value::Number(n) => n.to_string(),
                Value::Array(parts) => parts
                    .iter()
                    .map(|p| p.as_str().map(str::to_string).unwrap_or_else(|| p.to_string()))
                    .collect::<Vec<_>>()
                    .join("/"),
                other => bail!("{}: param `{k}` must be a string, got {other}", route.pattern),
            };
            if value.is_empty()
                || value.split('/').any(|p| p.is_empty() || p == "." || p == "..")
                || value.contains(['\\', '\0'])
            {
                bail!("{}: invalid value {value:?} for param `{k}`", route.pattern);
            }
            params.insert(k, value);
        }
        let path = routes::fill(&route.segments, &params)
            .with_context(|| format!("{}: staticParams() entry is missing a param", route.pattern))?;
        let ordered: routes::Params = route
            .segments
            .iter()
            .filter_map(|s| match s {
                routes::Segment::Param(n) | routes::Segment::CatchAll(n) => Some((n.clone(), params[n].clone())),
                routes::Segment::Static(_) => None,
            })
            .collect();
        out.push((path, ordered));
    }
    Ok(out)
}

/// Is `route` the one that serves `pathname`? A `staticParams()` entry such as `archive` for
/// `/blog/[slug]` is shadowed by a static `/blog/archive` page and must not overwrite it.
fn owns(routes: &[RouteEntry], route: &RouteEntry, pathname: &str) -> bool {
    let Some(parts) = routes::split_path(pathname) else { return false };
    routes
        .iter()
        .find(|r| r.kind == RouteKind::Page && routes::match_segments(&r.segments, &parts).is_some())
        .is_some_and(|r| r.id == route.id)
}

fn output_path(dist: &Path, pathname: &str, file: &str) -> PathBuf {
    let mut p = dist.to_path_buf();
    for part in pathname.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p.join(file)
}

fn copy_dir(from: &Path, to: &Path, out: &mut Vec<(PathBuf, u64)>) -> Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target, out)?;
        } else {
            std::fs::create_dir_all(to)?;
            let n = std::fs::copy(entry.path(), &target)?;
            out.push((target, n));
        }
    }
    Ok(())
}

async fn build_static(
    project: &Project,
    plan: &Plan,
    generated: &generated::Generated,
    server: &Bundle,
    server_code: Arc<str>,
    build_id: &str,
) -> Result<BuildResult> {
    let dist = project.dist_dir();
    let _ = std::fs::remove_dir_all(&dist);
    let client = with_server_assets(
        bundle::bundle(spec(project, ("client", &generated.client_entry), Side::Client)).await?,
        server,
    );
    let (assets, client_files) = write_client(&dist.join("_forge/static"), &client, generated)?;
    let manifest = manifest(project, plan, build_id, assets);
    let mut artifacts: Vec<(PathBuf, u64)> = Vec::new();
    copy_dir(&project.public_dir(), &dist, &mut artifacts)?;
    for f in &client_files {
        let p = dist.join("_forge/static").join(f);
        let n = std::fs::metadata(&p)?.len();
        artifacts.push((p, n));
    }

    let pool = build_pool(project, server_code.clone(), num_workers()).await?;
    let mut jobs = Vec::new();
    for route in manifest.routes.iter().filter(|r| r.kind == RouteKind::Page) {
        for (path, params) in concrete_paths(&pool, route).await? {
            if owns(&manifest.routes, route, &path) {
                jobs.push((route.clone(), path, params));
            }
        }
    }
    let rendered: Vec<Result<Files>> = futures_util::stream::iter(jobs)
        .map(|(route, path, params)| prerender_job(&pool, &manifest, &dist, route, path, params))
        .buffer_unordered(16)
        .collect()
        .await;
    for files in rendered {
        for (path, bytes) in files? {
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(&path, &bytes)?;
            artifacts.push((path, bytes.len() as u64));
        }
    }
    if manifest.not_found.is_some() {
        let html = prerender(&pool, &manifest, NOT_FOUND_ID, "/404", &Vec::new()).await?;
        let path = dist.join("404.html");
        std::fs::write(&path, &html)?;
        artifacts.push((path, html.len() as u64));
    }
    artifacts.sort();
    let rel: Vec<String> = artifacts
        .iter()
        .map(|(p, _)| p.strip_prefix(&project.root).unwrap_or(p).to_string_lossy().into_owned())
        .collect();
    let capsule = Capsule::of(build_id, &project.root, &rel)?;
    std::fs::write(project.out_dir().join("capsule.json"), serde_json::to_vec_pretty(&capsule)?)?;
    Ok(BuildResult {
        manifest,
        server_code,
        stats: BundleStats::of(Some(&client), server, &project.root),
        warnings: client.warnings,
        artifacts,
        elapsed: Duration::ZERO,
    })
}

/// Output files: `(path, contents)`.
type Files = Vec<(PathBuf, Vec<u8>)>;

async fn prerender_job(
    pool: &Pool,
    manifest: &Manifest,
    dist: &Path,
    route: RouteEntry,
    path: String,
    params: routes::Params,
) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut files =
        vec![(output_path(dist, &path, "index.html"), prerender(pool, manifest, &route.id, &path, &params).await?)];
    if route.loader {
        let (_, data, _) = call(pool, "data", &route.id, &path, &params).await?;
        let data_path = if path == "/" { "/index".to_string() } else { path.clone() };
        files.push((dist.join(format!("_forge/data{data_path}.json")), data));
    }
    Ok(files)
}

/// A complete HTML document for one page, rendered at build time.
async fn prerender(
    pool: &Pool,
    manifest: &Manifest,
    route: &str,
    pathname: &str,
    params: &routes::Params,
) -> Result<Vec<u8>> {
    let url = format!("http://localhost{pathname}");
    let req = render::Request {
        kind: "page",
        route,
        params: render::params_json(params),
        pathname,
        url: &url,
        method: "GET",
        headers: &[],
        nonce: "",
        stream: false,
    };
    let opts = render::PageOptions { nonce: None, live_reload: false };
    match render::page(pool, manifest, &req, &opts).await.map_err(|e| anyhow::anyhow!("{pathname}: {e}"))? {
        render::Rendered::Page { status, open, body } => {
            let body = body.collect().await;
            if status >= 500 {
                bail!("{pathname}: render failed");
            }
            let mut out = open.into_bytes();
            out.extend_from_slice(&body);
            out.extend_from_slice(html::Document::close().as_bytes());
            Ok(out)
        }
        render::Rendered::Raw { status, headers, body, .. } => {
            if let Some((_, location)) = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("location")) {
                let l = html::escape_html(location);
                return Ok(format!("<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"0;url={l}\"><link rel=\"canonical\" href=\"{l}\"><a href=\"{l}\">{l}</a>").into_bytes());
            }
            let body = body.collect().await;
            bail!("{pathname}: status {status}\n{}", strip_tags(&String::from_utf8_lossy(&body)))
        }
    }
}

async fn build_single(
    project: &Project,
    plan: &Plan,
    server: &Bundle,
    server_code: Arc<str>,
    build_id: &str,
    emit: impl Fn(Option<&str>) -> Result<generated::Generated>,
) -> Result<BuildResult> {
    let needs_engine = plan.routes.iter().any(|r| r.loader || (r.ssr && r.pattern == "/"));
    let pool = if needs_engine { Some(build_pool(project, server_code.clone(), 1).await?) } else { None };

    // Loader data is computed now and embedded: there is no server at runtime.
    let mut data = serde_json::Map::new();
    if let Some(pool) = &pool {
        for route in plan.routes.iter().filter(|r| r.loader) {
            for (path, params) in concrete_paths(pool, route).await? {
                if !owns(&plan.routes, route, &path) {
                    continue;
                }
                let (_, body, _) = call(pool, "data", &route.id, &path, &params).await?;
                let payload: Value = serde_json::from_slice(&body)?;
                if let Some(d) = payload.get("data") {
                    data.insert(path, d.clone());
                }
            }
        }
    }
    let static_data = Value::Object(data).to_string();
    let generated = emit(Some(&static_data))?;
    let client = bundle::bundle(spec(project, ("client", &generated.client_entry), Side::Client)).await?;
    let js = &client.entry("client").context("client bundle has no entry chunk")?.code;
    if client.chunks.len() > 1 {
        bail!(
            "single-html: the client bundle split into {} chunks; remove dynamic import() of app code",
            client.chunks.len()
        );
    }

    // Prerender `/` so the file shows content before the script runs.
    let home = plan.routes.iter().find(|r| r.kind == RouteKind::Page && r.pattern == "/");
    let (head_tags, boot, body) = match (home, &pool) {
        (Some(r), Some(pool)) if r.ssr => {
            let (_, body, page) = call(pool, "page", &r.id, "/", &Vec::new()).await?;
            let page = page.context("home page did not render as a page")?;
            (html::meta_tags(&page.meta, &project.config.title), page.boot, String::from_utf8_lossy(&body).into_owned())
        }
        _ => (html::meta_tags(&Default::default(), &project.config.title), "null".to_string(), String::new()),
    };
    let doc = html::SingleFile {
        lang: &project.config.lang,
        head_tags,
        css: &client.css,
        js,
        boot: &boot,
        body: &body,
        compress: project.config.build.compress,
    }
    .render();
    let dist = project.dist_dir();
    let _ = std::fs::remove_dir_all(&dist);
    std::fs::create_dir_all(&dist)?;
    let path = dist.join("index.html");
    std::fs::write(&path, &doc)?;
    let mut warnings = client.warnings.clone();
    if project.public_dir().is_dir() {
        warnings
            .push("single-html: files in public/ are not included; import assets from code so they are inlined".into());
    }
    if doc.len() > 10 << 20 {
        warnings.push(format!(
            "single-html: output is {:.1} MB; large assets make the file slow to open",
            doc.len() as f64 / 1048576.0
        ));
    }
    let rel = path.strip_prefix(&project.root).unwrap_or(&path).to_string_lossy().into_owned();
    let capsule = Capsule::of(build_id, &project.root, &[rel])?;
    std::fs::write(project.out_dir().join("capsule.json"), serde_json::to_vec_pretty(&capsule)?)?;
    let assets = ClientAssets { entry: "inline".into(), css: None, preload: BTreeMap::new() };
    Ok(BuildResult {
        manifest: manifest(project, plan, build_id, assets),
        server_code,
        stats: BundleStats::of(Some(&client), server, &project.root),
        warnings,
        artifacts: vec![(path, doc.len() as u64)],
        elapsed: Duration::ZERO,
    })
}

fn num_workers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get()).min(4)
}
