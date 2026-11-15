//! File-system routing: scan `app/`, order routes by specificity, match paths.
//!
//! Conventions (first existing extension wins, TypeScript first):
//!
//! ```text
//! app/page.{tsx,jsx,ts,js}        page (universal: server + client)
//! app/layout.{tsx,jsx,ts,js}      layout wrapping every page below it
//! app/loader.{ts,tsx,js,jsx}      server-only data loader for the page beside it
//! app/api/x/route.{ts,tsx,js,jsx} server-only HTTP endpoint
//! app/not-found.{tsx,jsx,ts,js}   404 page (app root only)
//! app/blog/[slug]/                dynamic segment
//! app/docs/[...path]/             catch-all (one or more segments)
//! app/(marketing)/about/          route group: not part of the URL
//! app/_components/                private: never scanned
//! ```

use std::{
    cmp::Ordering,
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

pub const COMPONENT_EXTS: &[&str] = &["tsx", "jsx", "ts", "js"];
pub const MODULE_EXTS: &[&str] = &["ts", "tsx", "js", "jsx"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "lowercase")]
pub enum Segment {
    Static(String),
    Param(String),
    CatchAll(String),
}

impl Segment {
    fn rank(&self) -> u8 {
        match self {
            Segment::Static(_) => 0,
            Segment::Param(_) => 1,
            Segment::CatchAll(_) => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteKind {
    Page,
    Api,
}

/// One routable directory. Paths are relative to the project root, `/`-separated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub id: String,
    /// Display form, e.g. `/blog/[slug]`.
    pub pattern: String,
    pub segments: Vec<Segment>,
    pub kind: RouteKind,
    /// `page.*` for pages, `route.*` for API routes.
    pub file: String,
    /// Outermost first.
    pub layouts: Vec<String>,
    pub loader: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RouteTable {
    /// Sorted most-specific first: the first match wins.
    pub routes: Vec<Route>,
    pub not_found: Option<String>,
    /// Layouts that apply to the not-found page (the root layout, if any).
    pub root_layouts: Vec<String>,
}

pub type Params = Vec<(String, String)>;

impl RouteTable {
    pub fn pages(&self) -> impl Iterator<Item = &Route> {
        self.routes.iter().filter(|r| r.kind == RouteKind::Page)
    }
    pub fn apis(&self) -> impl Iterator<Item = &Route> {
        self.routes.iter().filter(|r| r.kind == RouteKind::Api)
    }
}

/// Scan `app_dir`. `root` is only used to relativise file paths.
pub fn scan(root: &Path, app_dir: &Path) -> Result<RouteTable> {
    if !app_dir.is_dir() {
        bail!("app directory `{}` not found", app_dir.display());
    }
    let mut table = RouteTable {
        not_found: find(app_dir, "not-found", COMPONENT_EXTS).map(|p| rel(root, &p)),
        ..Default::default()
    };
    walk(root, app_dir, &mut Vec::new(), &mut Vec::new(), &mut table)?;
    table.root_layouts = find(app_dir, "layout", COMPONENT_EXTS).map(|p| rel(root, &p)).into_iter().collect();

    table.routes.sort_by(|a, b| specificity(&a.segments, &b.segments).then_with(|| a.pattern.cmp(&b.pattern)));

    // Two directories that resolve to the same URL shape are ambiguous.
    let mut seen: HashMap<String, &Route> = HashMap::new();
    for r in &table.routes {
        let shape: String = r
            .segments
            .iter()
            .map(|s| match s {
                Segment::Static(s) => format!("/{s}"),
                Segment::Param(_) => "/:".into(),
                Segment::CatchAll(_) => "/*".into(),
            })
            .collect();
        if let Some(prev) = seen.insert(shape, r) {
            bail!("routes `{}` and `{}` match the same URLs", prev.file, r.file);
        }
    }

    // Stable, unique, identifier-safe ids.
    let mut used: HashMap<String, usize> = HashMap::new();
    for r in &mut table.routes {
        let base = route_id(&r.segments);
        let n = used.entry(base.clone()).or_insert(0);
        r.id = if *n == 0 { base } else { format!("{base}_{n}") };
        *n += 1;
    }
    Ok(table)
}

fn walk(
    root: &Path,
    dir: &Path,
    segments: &mut Vec<Segment>,
    layouts: &mut Vec<String>,
    table: &mut RouteTable,
) -> Result<()> {
    let layout = find(dir, "layout", COMPONENT_EXTS).map(|p| rel(root, &p));
    if let Some(l) = &layout {
        layouts.push(l.clone());
    }

    let page = find(dir, "page", COMPONENT_EXTS);
    let api = find(dir, "route", MODULE_EXTS);
    let loader = find(dir, "loader", MODULE_EXTS);
    if let (Some(p), Some(a)) = (&page, &api) {
        bail!("`{}` and `{}` cannot share a directory", rel(root, p), rel(root, a));
    }
    if let (None, Some(l)) = (&page, &loader) {
        bail!("`{}` has no page beside it", rel(root, l));
    }
    let pattern = pattern_of(segments);
    if let Some(p) = page {
        table.routes.push(Route {
            id: String::new(),
            pattern,
            segments: segments.clone(),
            kind: RouteKind::Page,
            file: rel(root, &p),
            layouts: layouts.clone(),
            loader: loader.map(|l| rel(root, &l)),
        });
    } else if let Some(a) = api {
        table.routes.push(Route {
            id: String::new(),
            pattern,
            segments: segments.clone(),
            kind: RouteKind::Api,
            file: rel(root, &a),
            layouts: Vec::new(),
            loader: None,
        });
    }

    let mut children: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    children.sort();
    for child in children {
        let name = child.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.starts_with('_') || name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let segment = parse_segment(name).with_context(|| format!("in {}", rel(root, &child)))?;
        if let Some(Segment::CatchAll(_)) = segments.last() {
            bail!("`{}`: a catch-all segment must be the last segment", rel(root, &child));
        }
        let pushed = segment.is_some();
        segments.extend(segment);
        walk(root, &child, segments, layouts, table)?;
        if pushed {
            segments.pop();
        }
    }

    if layout.is_some() {
        layouts.pop();
    }
    Ok(())
}

/// `None` for route groups, which contribute no URL segment.
fn parse_segment(name: &str) -> Result<Option<Segment>> {
    if name.starts_with('(') && name.ends_with(')') {
        return Ok(None);
    }
    if let Some(inner) = name.strip_prefix('[').and_then(|n| n.strip_suffix(']')) {
        let (catch_all, ident) = match inner.strip_prefix("...") {
            Some(rest) => (true, rest),
            None => (false, inner),
        };
        if ident.is_empty() || !ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("invalid dynamic segment `{name}`");
        }
        return Ok(Some(if catch_all { Segment::CatchAll(ident.into()) } else { Segment::Param(ident.into()) }));
    }
    Ok(Some(Segment::Static(name.to_string())))
}

fn find(dir: &Path, stem: &str, exts: &[&str]) -> Option<PathBuf> {
    exts.iter().map(|ext| dir.join(format!("{stem}.{ext}"))).find(|p| p.is_file())
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn pattern_of(segments: &[Segment]) -> String {
    if segments.is_empty() {
        return "/".into();
    }
    segments
        .iter()
        .map(|s| match s {
            Segment::Static(s) => format!("/{s}"),
            Segment::Param(p) => format!("/[{p}]"),
            Segment::CatchAll(p) => format!("/[...{p}]"),
        })
        .collect()
}

fn route_id(segments: &[Segment]) -> String {
    if segments.is_empty() {
        return "route_home".into();
    }
    let mut id = String::from("route");
    for s in segments {
        let (Segment::Static(name) | Segment::Param(name) | Segment::CatchAll(name)) = s;
        id.push('_');
        id.extend(name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }));
    }
    id
}

/// Static beats dynamic beats catch-all, segment by segment.
fn specificity(a: &[Segment], b: &[Segment]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        match x.rank().cmp(&y.rank()) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    b.len().cmp(&a.len())
}

/// Split a URL path into decoded segments. `None` if it is not valid UTF-8 or tries to traverse.
pub fn split_path(path: &str) -> Option<Vec<String>> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let decoded = percent_decode_str(s).decode_utf8().ok()?;
            (decoded != "." && decoded != ".." && !decoded.contains(['/', '\\', '\0'])).then(|| decoded.into_owned())
        })
        .collect()
}

pub fn match_segments(segments: &[Segment], parts: &[String]) -> Option<Params> {
    let mut params = Params::new();
    for (i, seg) in segments.iter().enumerate() {
        match seg {
            Segment::Static(s) => {
                if parts.get(i)? != s {
                    return None;
                }
            }
            Segment::Param(name) => params.push((name.clone(), parts.get(i)?.clone())),
            Segment::CatchAll(name) => {
                if parts.len() <= i {
                    return None;
                }
                params.push((name.clone(), parts[i..].join("/")));
                return Some(params);
            }
        }
    }
    (parts.len() == segments.len()).then_some(params)
}

/// Fill a route pattern with concrete params (used when prerendering).
pub fn fill(segments: &[Segment], params: &HashMap<String, String>) -> Option<String> {
    if segments.is_empty() {
        return Some("/".into());
    }
    let mut out = String::new();
    for seg in segments {
        out.push('/');
        match seg {
            Segment::Static(s) => out.push_str(s),
            Segment::Param(n) | Segment::CatchAll(n) => out.push_str(params.get(n)?),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, file: &str) {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "export default 1;").unwrap();
    }

    fn fixture(name: &str, files: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("forge-routes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in files {
            touch(&root, f);
        }
        root
    }

    fn scan_fixture(name: &str, files: &[&str]) -> Result<RouteTable> {
        let root = fixture(name, files);
        let out = scan(&root, &root.join("app"));
        let _ = std::fs::remove_dir_all(&root);
        out
    }

    fn parts(p: &str) -> Vec<String> {
        split_path(p).unwrap()
    }

    #[test]
    fn scans_pages_layouts_loaders_and_api() {
        let t = scan_fixture(
            "basic",
            &[
                "app/page.tsx",
                "app/layout.tsx",
                "app/not-found.jsx",
                "app/blog/layout.jsx",
                "app/blog/[slug]/page.jsx",
                "app/blog/[slug]/loader.ts",
                "app/api/chat/route.ts",
                "app/(marketing)/about/page.js",
                "app/_private/page.tsx",
                "app/docs/[...path]/page.tsx",
            ],
        )
        .unwrap();
        let patterns: Vec<_> = t.routes.iter().map(|r| r.pattern.as_str()).collect();
        assert_eq!(patterns, ["/api/chat", "/blog/[slug]", "/docs/[...path]", "/about", "/"]);
        let blog = t.routes.iter().find(|r| r.pattern == "/blog/[slug]").unwrap();
        assert_eq!(blog.id, "route_blog_slug");
        assert_eq!(blog.layouts, ["app/layout.tsx", "app/blog/layout.jsx"]);
        assert_eq!(blog.loader.as_deref(), Some("app/blog/[slug]/loader.ts"));
        assert_eq!(t.routes.iter().find(|r| r.pattern == "/").unwrap().id, "route_home");
        assert_eq!(t.not_found.as_deref(), Some("app/not-found.jsx"));
        assert_eq!(t.apis().count(), 1);
    }

    #[test]
    fn typescript_wins_when_both_exist() {
        let t =
            scan_fixture("prec", &["app/page.jsx", "app/page.tsx", "app/page.js", "app/layout.js", "app/layout.jsx"])
                .unwrap();
        assert_eq!(t.routes[0].file, "app/page.tsx");
        assert_eq!(t.routes[0].layouts, ["app/layout.jsx"]);
    }

    #[test]
    fn static_segments_outrank_dynamic_ones() {
        let t = scan_fixture(
            "order",
            &["app/[id]/page.tsx", "app/new/page.tsx", "app/[...rest]/page.tsx", "app/[id]/edit/page.tsx"],
        )
        .unwrap();
        let patterns: Vec<_> = t.routes.iter().map(|r| r.pattern.as_str()).collect();
        assert_eq!(patterns, ["/new", "/[id]/edit", "/[id]", "/[...rest]"]);
    }

    #[test]
    fn rejects_ambiguous_and_invalid_layouts() {
        assert!(scan_fixture("amb", &["app/[a]/page.tsx", "app/[b]/page.tsx"]).is_err());
        assert!(scan_fixture("grp", &["app/(x)/a/page.tsx", "app/(y)/a/page.tsx"]).is_err());
        assert!(scan_fixture("both", &["app/x/page.tsx", "app/x/route.ts"]).is_err());
        assert!(scan_fixture("orphan", &["app/x/loader.ts"]).is_err());
        assert!(scan_fixture("tail", &["app/[...a]/b/page.tsx"]).is_err());
    }

    #[test]
    fn matching() {
        let blog = [Segment::Static("blog".into()), Segment::Param("slug".into())];
        assert_eq!(
            match_segments(&blog, &parts("/blog/hello%20world")),
            Some(vec![("slug".into(), "hello world".into())])
        );
        assert_eq!(match_segments(&blog, &parts("/blog")), None);
        assert_eq!(match_segments(&blog, &parts("/blog/a/b")), None);
        let docs = [Segment::Static("docs".into()), Segment::CatchAll("path".into())];
        assert_eq!(match_segments(&docs, &parts("/docs/a/b/")), Some(vec![("path".into(), "a/b".into())]));
        assert_eq!(match_segments(&docs, &parts("/docs")), None);
        assert_eq!(match_segments(&[], &parts("/")), Some(vec![]));
    }

    #[test]
    fn split_path_rejects_traversal() {
        assert!(split_path("/a/../b").is_none());
        assert!(split_path("/a/%2e%2e/b").is_none());
        assert!(split_path("/a/b%2Fc").is_none());
        assert_eq!(split_path("//a//b/").unwrap(), ["a", "b"]);
    }

    #[test]
    fn fill_pattern() {
        let blog = [Segment::Static("blog".into()), Segment::Param("slug".into())];
        let params = HashMap::from([("slug".to_string(), "x".to_string())]);
        assert_eq!(fill(&blog, &params).as_deref(), Some("/blog/x"));
        assert_eq!(fill(&blog, &HashMap::new()), None);
    }
}
