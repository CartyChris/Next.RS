//! Code generation: the entry modules Rolldown bundles, plus the embedded JS runtime.
//!
//! ```text
//! .forge/generated/
//!   server-entry.js              every page, layout, loader and API route, statically imported
//!   client-entry.js              route table with one dynamic import per route
//!   routes/<route id>.client.js  page + its layout chain (a code-split chunk)
//!   runtime/*.js                 Forge's own runtime, written from the binary
//! ```

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{
    config::{Mode, Target},
    routes::{Route, RouteTable, Segment},
};

/// Runtime sources embedded in the binary: `(file name, source)`.
pub const RUNTIME: &[(&str, &str)] = &[
    ("shared.js", include_str!("../runtime/js/shared.js")),
    ("client.js", include_str!("../runtime/js/client.js")),
    ("server.js", include_str!("../runtime/js/server.js")),
    ("host-ref.js", include_str!("../runtime/js/host-ref.js")),
    ("react.js", include_str!("../runtime/js/react.js")),
    ("react-client.js", include_str!("../runtime/js/react-client.js")),
    ("react-server.js", include_str!("../runtime/js/react-server.js")),
    ("navigation.js", include_str!("../runtime/js/navigation.js")),
    ("data.js", include_str!("../runtime/js/data.js")),
    ("cache.server.js", include_str!("../runtime/js/cache.server.js")),
    ("cache.client.js", include_str!("../runtime/js/cache.client.js")),
    ("store.js", include_str!("../runtime/js/store.js")),
    ("ai.js", include_str!("../runtime/js/ai.js")),
    ("game.js", include_str!("../runtime/js/game.js")),
    ("realtime.server.js", include_str!("../runtime/js/realtime.server.js")),
    ("realtime.client.js", include_str!("../runtime/js/realtime.client.js")),
];

/// Public virtual modules: `(specifier, server file, client file)`.
pub const VIRTUAL_MODULES: &[(&str, &str, &str)] = &[
    ("forge:navigation", "navigation.js", "navigation.js"),
    ("forge:data", "data.js", "data.js"),
    ("forge:cache", "cache.server.js", "cache.client.js"),
    ("forge:store", "store.js", "store.js"),
    ("forge:ai", "ai.js", "ai.js"),
    ("forge:game", "game.js", "game.js"),
    ("forge:realtime", "realtime.server.js", "realtime.client.js"),
];

pub const NOT_FOUND_ID: &str = "not_found";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageKind {
    /// Default export is a React component.
    React,
    /// Exports `mount(el, ctx)` and/or `render(ctx)`; no framework.
    Vanilla,
}

/// What the code generator needs to know about each page (from static analysis).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageInfo {
    pub kind: PageKind,
    /// Rendered on the server. Client-only pages are not even imported by the server bundle,
    /// so their modules may touch `window`/`document` at the top level.
    pub ssr: bool,
}

impl PageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PageKind::React => "react",
            PageKind::Vanilla => "vanilla",
        }
    }
}

/// How the client reaches loader data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataMode {
    /// `GET /_forge/data?path=…` against a running server.
    Server,
    /// Prebuilt `/_forge/data/<path>.json` files.
    Static,
    /// Embedded in the bundle (single-file HTML).
    Inline,
}

impl DataMode {
    pub fn for_target(target: Target) -> Self {
        match target {
            Target::Server => DataMode::Server,
            Target::Static => DataMode::Static,
            Target::SingleHtml | Target::WasmHost => DataMode::Inline,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            DataMode::Server => "server",
            DataMode::Static => "static",
            DataMode::Inline => "inline",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Generated {
    pub dir: PathBuf,
    pub server_entry: PathBuf,
    pub client_entry: PathBuf,
    /// `(route id, generated client module)`.
    pub route_modules: Vec<(String, PathBuf)>,
}

pub struct EmitInput<'a> {
    pub root: &'a Path,
    pub dir: &'a Path,
    pub table: &'a RouteTable,
    /// Per route id (including [`NOT_FOUND_ID`]).
    pub pages: &'a BTreeMap<String, PageInfo>,
    pub mode: Mode,
    pub target: Target,
    /// single-html: JSON object of `pathname → loader data`, computed at build time.
    pub static_data: Option<&'a str>,
}

pub fn emit(input: &EmitInput<'_>) -> Result<Generated> {
    let dir = input.dir;
    let runtime = dir.join("runtime");
    let routes_dir = dir.join("routes");
    std::fs::create_dir_all(&runtime)?;
    // Route modules are regenerated from scratch so deleted routes do not linger.
    let _ = std::fs::remove_dir_all(&routes_dir);
    std::fs::create_dir_all(&routes_dir)?;
    for (name, source) in RUNTIME {
        write_if_changed(&runtime.join(name), source)?;
    }

    let mut route_modules = Vec::new();
    for route in input.table.pages() {
        let path = routes_dir.join(format!("{}.client.js", route.id));
        write_if_changed(&path, &client_route_module(input.root, &routes_dir, &route.file, &route.layouts))?;
        route_modules.push((route.id.clone(), path));
    }
    if let Some(file) = &input.table.not_found {
        let path = routes_dir.join(format!("{NOT_FOUND_ID}.client.js"));
        write_if_changed(&path, &client_route_module(input.root, &routes_dir, file, &input.table.root_layouts))?;
        route_modules.push((NOT_FOUND_ID.into(), path));
    }

    let server_entry = dir.join("server-entry.js");
    let client_entry = dir.join("client-entry.js");
    write_if_changed(&server_entry, &server_entry_source(input))?;
    write_if_changed(&client_entry, &client_entry_source(input))?;
    Ok(Generated { dir: dir.to_path_buf(), server_entry, client_entry, route_modules })
}

fn uses_react(input: &EmitInput<'_>) -> bool {
    input.pages.values().any(|p| p.kind == PageKind::React)
}

fn info(input: &EmitInput<'_>, id: &str) -> PageInfo {
    input.pages.get(id).copied().unwrap_or(PageInfo { kind: PageKind::React, ssr: true })
}

fn kind_of(input: &EmitInput<'_>, id: &str) -> &'static str {
    info(input, id).kind.as_str()
}

fn client_route_module(root: &Path, from_dir: &Path, page: &str, layouts: &[String]) -> String {
    let mut out = format!("import * as page from {};\n", js_str(&relative_import(from_dir, &root.join(page))));
    for (i, layout) in layouts.iter().enumerate() {
        let _ = writeln!(out, "import * as l{i} from {};", js_str(&relative_import(from_dir, &root.join(layout))));
    }
    let names: Vec<String> = (0..layouts.len()).map(|i| format!("l{i}")).collect();
    let _ = write!(out, "export {{ page }};\nexport const layouts = [{}];\n", names.join(", "));
    out
}

/// Deduplicating import table: each source file becomes one `import * as mN`.
struct Imports<'a> {
    root: &'a Path,
    from_dir: &'a Path,
    names: BTreeMap<String, String>,
    code: String,
}

impl<'a> Imports<'a> {
    fn new(root: &'a Path, from_dir: &'a Path) -> Self {
        Self { root, from_dir, names: BTreeMap::new(), code: String::new() }
    }
    fn name(&mut self, file: &str) -> String {
        if let Some(n) = self.names.get(file) {
            return n.clone();
        }
        let name = format!("m{}", self.names.len());
        let _ = writeln!(
            self.code,
            "import * as {name} from {};",
            js_str(&relative_import(self.from_dir, &self.root.join(file)))
        );
        self.names.insert(file.to_string(), name.clone());
        name
    }
}

fn server_entry_source(input: &EmitInput<'_>) -> String {
    let mut imports = Imports::new(input.root, input.dir);
    let page_entry = |imports: &mut Imports<'_>, id: &str, file: &str, layouts: &[String], loader: Option<&String>| {
        let loader = loader.map_or("null".to_string(), |l| imports.name(l));
        let module = if info(input, id).ssr {
            let page = imports.name(file);
            let layouts: Vec<String> = layouts.iter().map(|l| imports.name(l)).collect();
            format!("{{ page: {page}, layouts: [{}] }}", layouts.join(", "))
        } else {
            "null".to_string()
        };
        format!("{{ id: {}, kind: {}, mod: {module}, loader: {loader} }}", js_str(id), js_str(kind_of(input, id)))
    };

    let mut routes = Vec::new();
    for r in input.table.pages() {
        routes.push(page_entry(&mut imports, &r.id, &r.file, &r.layouts, r.loader.as_ref()));
    }
    let not_found = match &input.table.not_found {
        Some(file) => page_entry(&mut imports, NOT_FOUND_ID, file, &input.table.root_layouts, None),
        None => "null".into(),
    };
    let apis: Vec<String> =
        input.table.apis().map(|r| format!("{{ id: {}, mod: {} }}", js_str(&r.id), imports.name(&r.file))).collect();

    // The runtime import comes first so it claims the host ops before any app module runs.
    let mut out = String::from("import { createServer } from \"./runtime/server.js\";\n");
    let renderers = if uses_react(input) {
        out.push_str("import { render as renderReact } from \"./runtime/react-server.js\";\n");
        "{ react: renderReact }"
    } else {
        "{}"
    };
    out.push_str(&imports.code);
    let _ = write!(
        out,
        "\ncreateServer({{\n  dev: {},\n  renderers: {renderers},\n  routes: [\n    {}\n  ],\n  notFound: {not_found},\n  apis: [{}],\n}});\n",
        input.mode.is_dev(),
        routes.join(",\n    "),
        apis.join(", "),
    );
    out
}

fn client_entry_source(input: &EmitInput<'_>) -> String {
    let entry = |id: &str, segments: Option<&[Segment]>, has_loader: bool| {
        format!(
            "{{ id: {}, segs: {}, kind: {}, loader: {has_loader}, load: () => import({}) }}",
            js_str(id),
            segments.map_or("null".to_string(), segments_js),
            js_str(kind_of(input, id)),
            js_str(&format!("./routes/{id}.client.js")),
        )
    };
    let routes: Vec<String> =
        input.table.pages().map(|r: &Route| entry(&r.id, Some(&r.segments), r.loader.is_some())).collect();
    let not_found = if input.table.not_found.is_some() { entry(NOT_FOUND_ID, None, false) } else { "null".into() };

    let mut out = String::from("import { start } from \"./runtime/client.js\";\n");
    let adapters = if uses_react(input) {
        out.push_str("import * as react from \"./runtime/react-client.js\";\n");
        "{ react }"
    } else {
        "{}"
    };
    let _ = write!(
        out,
        "\nstart({{\n  mode: {},\n  data: {},\n  adapters: {adapters},\n  routes: [\n    {}\n  ],\n  notFound: {not_found},\n  staticData: {},\n}});\n",
        js_str(if input.target == Target::SingleHtml { "hash" } else { "history" }),
        js_str(DataMode::for_target(input.target).as_str()),
        routes.join(",\n    "),
        input.static_data.unwrap_or("null"),
    );
    out
}

/// `[[0,"blog"],[1,"slug"]]`: 0 static, 1 param, 2 catch-all. Mirrors `routes::match_segments`.
pub fn segments_js(segments: &[Segment]) -> String {
    let parts: Vec<String> = segments
        .iter()
        .map(|s| match s {
            Segment::Static(v) => format!("[0,{}]", js_str(v)),
            Segment::Param(v) => format!("[1,{}]", js_str(v)),
            Segment::CatchAll(v) => format!("[2,{}]", js_str(v)),
        })
        .collect();
    format!("[{}]", parts.join(","))
}

/// A JS string literal (JSON is a subset of JS string syntax).
pub fn js_str(s: &str) -> String {
    serde_json::to_string(s).expect("string serialization cannot fail")
}

/// Import specifier for `to_file` as seen from `from_dir`: always `./` or `../` prefixed, `/`-separated.
pub fn relative_import(from_dir: &Path, to_file: &Path) -> String {
    let from: Vec<Component<'_>> = from_dir.components().collect();
    let to: Vec<Component<'_>> = to_file.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = std::iter::repeat_n("..".to_string(), from.len() - common).collect();
    parts.extend(to[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    let joined = parts.join("/");
    if joined.starts_with("../") { joined } else { format!("./{joined}") }
}

/// Skip the write when nothing changed, so file watchers and mtime-based caches stay quiet.
pub fn write_if_changed(path: &Path, contents: &str) -> Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::RouteKind;

    #[test]
    fn relative_imports() {
        let gen_dir = Path::new("/p/.forge/generated");
        assert_eq!(relative_import(gen_dir, Path::new("/p/app/page.tsx")), "../../app/page.tsx");
        assert_eq!(
            relative_import(&gen_dir.join("routes"), Path::new("/p/app/blog/[slug]/page.jsx")),
            "../../../app/blog/[slug]/page.jsx"
        );
        assert_eq!(relative_import(gen_dir, Path::new("/p/.forge/generated/runtime/client.js")), "./runtime/client.js");
    }

    #[test]
    fn js_strings_are_escaped() {
        assert_eq!(js_str("a\"b\\c\n"), r#""a\"b\\c\n""#);
    }

    fn table() -> RouteTable {
        let page = |id: &str, pattern: &str, segments: Vec<Segment>, file: &str, loader: Option<&str>| Route {
            id: id.into(),
            pattern: pattern.into(),
            segments,
            kind: RouteKind::Page,
            file: file.into(),
            layouts: vec!["app/layout.tsx".into()],
            loader: loader.map(Into::into),
        };
        RouteTable {
            routes: vec![
                Route {
                    id: "route_api_chat".into(),
                    pattern: "/api/chat".into(),
                    segments: vec![],
                    kind: RouteKind::Api,
                    file: "app/api/chat/route.ts".into(),
                    layouts: vec![],
                    loader: None,
                },
                page(
                    "route_blog_slug",
                    "/blog/[slug]",
                    vec![Segment::Static("blog".into()), Segment::Param("slug".into())],
                    "app/blog/[slug]/page.tsx",
                    Some("app/blog/[slug]/loader.ts"),
                ),
                page("route_home", "/", vec![], "app/page.tsx", None),
            ],
            not_found: Some("app/not-found.tsx".into()),
            root_layouts: vec!["app/layout.tsx".into()],
        }
    }

    fn emit_fixture(name: &str, pages: &BTreeMap<String, PageInfo>, target: Target) -> (PathBuf, Generated) {
        let root = std::env::temp_dir().join(format!("forge-gen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join(".forge/generated");
        let table = table();
        let generated = emit(&EmitInput {
            root: &root,
            dir: &dir,
            table: &table,
            pages,
            mode: Mode::Prod,
            target,
            static_data: None,
        })
        .unwrap();
        (root, generated)
    }

    #[test]
    fn emits_server_and_client_entries() {
        let pages = BTreeMap::from([
            ("route_home".to_string(), PageInfo { kind: PageKind::React, ssr: true }),
            ("route_blog_slug".to_string(), PageInfo { kind: PageKind::React, ssr: true }),
        ]);
        let (root, generated) = emit_fixture("entries", &pages, Target::Server);
        let server = std::fs::read_to_string(&generated.server_entry).unwrap();
        let client = std::fs::read_to_string(&generated.client_entry).unwrap();
        let route = std::fs::read_to_string(root.join(".forge/generated/routes/route_blog_slug.client.js")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        // Runtime first (it claims the host), every source imported once.
        assert!(server.starts_with("import { createServer } from \"./runtime/server.js\";"));
        assert_eq!(server.matches("\"../../app/layout.tsx\"").count(), 1);
        assert!(server.contains("\"../../app/blog/[slug]/loader.ts\""));
        assert!(server.contains("\"../../app/api/chat/route.ts\""));
        assert!(server.contains("renderers: { react: renderReact }"));

        // The client never references server-only files.
        assert!(!client.contains("loader.ts") && !client.contains("route.ts"));
        assert!(client.contains(r#"{ id: "route_blog_slug", segs: [[0,"blog"],[1,"slug"]], kind: "react", loader: true, load: () => import("./routes/route_blog_slug.client.js") }"#));
        assert!(client.contains(r#"mode: "history""#) && client.contains(r#"data: "server""#));
        assert!(client.contains(r#"{ id: "not_found", segs: null"#));

        assert_eq!(
            route,
            "import * as page from \"../../../app/blog/[slug]/page.tsx\";\nimport * as l0 from \"../../../app/layout.tsx\";\nexport { page };\nexport const layouts = [l0];\n"
        );
        assert_eq!(generated.route_modules.len(), 3);
    }

    #[test]
    fn vanilla_apps_do_not_import_react() {
        let pages: BTreeMap<String, PageInfo> = ["route_home", "route_blog_slug", NOT_FOUND_ID]
            .iter()
            .map(|id| (id.to_string(), PageInfo { kind: PageKind::Vanilla, ssr: *id != "route_home" }))
            .collect();
        let (root, generated) = emit_fixture("vanilla", &pages, Target::SingleHtml);
        let server = std::fs::read_to_string(&generated.server_entry).unwrap();
        let client = std::fs::read_to_string(&generated.client_entry).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(!server.contains("react") && !client.contains("react-client"));
        // Client-only pages are not imported by the server bundle at all.
        assert!(server.contains(r#"{ id: "route_home", kind: "vanilla", mod: null, loader: null }"#));
        assert!(!server.contains("\"../../app/page.tsx\""));
        assert!(client.contains(r#"mode: "hash""#) && client.contains(r#"data: "inline""#));
    }
}
