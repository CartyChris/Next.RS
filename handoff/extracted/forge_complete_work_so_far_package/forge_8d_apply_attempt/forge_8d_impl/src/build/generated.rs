use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{BuildConfig, BundleEntry, BundleTarget};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteSpec {
    pub route: String,
    pub source_path: PathBuf,
    pub is_layout: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedBuildGraph {
    pub generated_dir: PathBuf,
    pub server_entry: GeneratedEntry,
    pub client_runtime_entry: GeneratedEntry,
    pub route_entries: Vec<GeneratedRouteEntry>,
    pub layout_entries: Vec<GeneratedLayoutEntry>,
    pub internal_entries: Vec<GeneratedEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedEntry {
    pub entry_name: String,
    pub path: PathBuf,
    pub target: BundleTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedRouteEntry {
    pub route: String,
    pub source_path: PathBuf,
    pub server: GeneratedEntry,
    pub client: GeneratedEntry,
    pub has_loader: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedLayoutEntry {
    pub route_prefix: String,
    pub source_path: PathBuf,
    pub server: GeneratedEntry,
    pub client: GeneratedEntry,
    pub depth: usize,
}

#[derive(Debug, Error)]
pub enum GeneratedBuildError {
    #[error("failed to create directory `{path}`: {source}")]
    CreateDir { path: PathBuf, source: std::io::Error },

    #[error("failed to write file `{path}`: {source}")]
    WriteFile { path: PathBuf, source: std::io::Error },

    #[error("failed to read file `{path}`: {source}")]
    ReadFile { path: PathBuf, source: std::io::Error },
}

impl GeneratedBuildGraph {
    pub fn bundle_entries(&self) -> Vec<BundleEntry> {
        let mut entries = Vec::new();

        entries.push(BundleEntry {
            name: self.server_entry.entry_name.clone(),
            input: self.server_entry.path.clone(),
            target: self.server_entry.target.clone(),
        });

        entries.push(BundleEntry {
            name: self.client_runtime_entry.entry_name.clone(),
            input: self.client_runtime_entry.path.clone(),
            target: self.client_runtime_entry.target.clone(),
        });

        for route in &self.route_entries {
            entries.push(BundleEntry {
                name: route.client.entry_name.clone(),
                input: route.client.path.clone(),
                target: route.client.target.clone(),
            });
        }

        for layout in &self.layout_entries {
            entries.push(BundleEntry {
                name: layout.client.entry_name.clone(),
                input: layout.client.path.clone(),
                target: layout.client.target.clone(),
            });
        }

        entries
    }
}

pub fn emit_generated_build_graph(
    config: &BuildConfig,
    specs: &[RouteSpec],
) -> Result<GeneratedBuildGraph, GeneratedBuildError> {
    let generated_dir = config.output_dir.join("generated");
    let routes_dir = generated_dir.join("routes");
    let layouts_dir = generated_dir.join("layouts");
    let internals_dir = generated_dir.join("internals");

    create_dir(&routes_dir)?;
    create_dir(&layouts_dir)?;
    create_dir(&internals_dir)?;

    emit_internal_runtime_modules(&internals_dir)?;

    let mut route_entries = Vec::new();
    let mut layout_entries = Vec::new();

    for spec in specs {
        if spec.is_layout {
            layout_entries.push(emit_layout_entry(spec, &layouts_dir)?);
        } else {
            route_entries.push(emit_route_entry(spec, &routes_dir)?);
        }
    }

    let server_entry = GeneratedEntry {
        entry_name: "server-entry".to_string(),
        path: generated_dir.join("server-entry.js"),
        target: BundleTarget::Server,
    };

    let client_runtime_entry = GeneratedEntry {
        entry_name: "forge-client-runtime".to_string(),
        path: generated_dir.join("client-runtime.js"),
        target: BundleTarget::Browser,
    };

    write_file(
        &server_entry.path,
        generate_server_entry_source(&route_entries, &layout_entries),
    )?;

    write_file(
        &client_runtime_entry.path,
        "console.log('forge client runtime');\n".to_string(),
    )?;

    Ok(GeneratedBuildGraph {
        generated_dir,
        server_entry,
        client_runtime_entry,
        route_entries,
        layout_entries,
        internal_entries: vec![],
    })
}

fn emit_route_entry(
    spec: &RouteSpec,
    routes_dir: &Path,
) -> Result<GeneratedRouteEntry, GeneratedBuildError> {
    let safe_name = safe_entry_name(&spec.route);
    let source = read_file(&spec.source_path)?;
    let has_loader = source_has_loader_export(&source);

    let server = GeneratedEntry {
        entry_name: format!("route-{safe_name}-server"),
        path: routes_dir.join(format!("route-{safe_name}.server.js")),
        target: BundleTarget::Server,
    };

    let client = GeneratedEntry {
        entry_name: format!("route-{safe_name}"),
        path: routes_dir.join(format!("route-{safe_name}.client.js")),
        target: BundleTarget::Browser,
    };

    let server_import = relative_import_specifier(
        server.path.parent().expect("server entry parent"),
        &spec.source_path,
    );

    let client_import = relative_import_specifier(
        client.path.parent().expect("client entry parent"),
        &spec.source_path,
    );

    write_file(
        &server.path,
        generate_server_route_wrapper(&spec.route, &server_import),
    )?;

    write_file(
        &client.path,
        generate_client_route_wrapper(&spec.route, &client_import),
    )?;

    Ok(GeneratedRouteEntry {
        route: spec.route.clone(),
        source_path: spec.source_path.clone(),
        server,
        client,
        has_loader,
    })
}

fn emit_layout_entry(
    spec: &RouteSpec,
    layouts_dir: &Path,
) -> Result<GeneratedLayoutEntry, GeneratedBuildError> {
    let safe_name = safe_entry_name(&spec.route);
    let depth = spec.route.trim_matches('/').split('/').filter(|part| !part.is_empty()).count();

    let server = GeneratedEntry {
        entry_name: format!("layout-{safe_name}-server"),
        path: layouts_dir.join(format!("layout-{safe_name}.server.js")),
        target: BundleTarget::Server,
    };

    let client = GeneratedEntry {
        entry_name: format!("layout-{safe_name}"),
        path: layouts_dir.join(format!("layout-{safe_name}.client.js")),
        target: BundleTarget::Browser,
    };

    let client_import = relative_import_specifier(
        client.path.parent().expect("layout client parent"),
        &spec.source_path,
    );

    write_file(&server.path, format!("export const routePrefix = {:?};\n", spec.route))?;
    write_file(&client.path, format!("import Component from {:?};\nexport default Component;\n", client_import))?;

    Ok(GeneratedLayoutEntry {
        route_prefix: spec.route.clone(),
        source_path: spec.source_path.clone(),
        server,
        client,
        depth,
    })
}

pub fn generate_server_route_wrapper(route: &str, impl_import: &str) -> String {
    format!(
        r#"
import Component from {impl_import:?};
import * as RouteModule from {impl_import:?};

export const route = {route:?};
export const Loading = RouteModule.Loading ?? null;
export const ErrorBoundary = RouteModule.ErrorBoundary ?? null;
export const loader = RouteModule.loader ?? null;

export async function load(ctx) {{
  if (!loader) return {{}};
  return await loader({{
    ctx,
    route,
    params: ctx.params ?? {{}},
    searchParams: ctx.searchParams ?? {{}},
    signal: ctx.signal,
  }});
}}

export function renderElement(ctx) {{
  return Component({{
    ctx,
    route,
    params: ctx.params ?? {{}},
    searchParams: ctx.searchParams ?? {{}},
  }});
}}

export {{ Component }};
export default Component;
"#
    )
}

pub fn generate_client_route_wrapper(route: &str, impl_import: &str) -> String {
    format!(
        r#"
import Component from {impl_import:?};
import * as RouteModule from {impl_import:?};

export const route = {route:?};
export const Loading = RouteModule.Loading ?? null;
export const ErrorBoundary = RouteModule.ErrorBoundary ?? null;
export const clientLoader = RouteModule.clientLoader ?? null;

export {{ Component }};
export default Component;
"#
    )
}

pub fn generate_server_entry_source(
    routes: &[GeneratedRouteEntry],
    layouts: &[GeneratedLayoutEntry],
) -> String {
    let imports = routes
        .iter()
        .enumerate()
        .map(|(index, route)| {
            let import_path = relative_import_specifier(
                Path::new("."),
                &route.server.path,
            );
            format!("import * as route_{index} from {import_path:?};")
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"
{imports}

const ROUTES = [{}];

globalThis.__forgeLoadRouteData = async function(ctx) {{
  return {{
    schemaVersion: 1,
    kind: "route_data",
    ok: true,
    status: 200,
    routes: ROUTES.length,
    props: {{}},
    data: {{ records: [] }},
    cache: {{ maxAgeMs: 0, staleWhileRevalidateMs: 0 }}
  }};
}};

globalThis.__forgeRenderToString = async function(ctx) {{
  return "<!doctype html><html><body><div id=\"__forge_root\"></div></body></html>";
}};
"#,
        routes
            .iter()
            .map(|route| format!("{:?}", route.route))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub fn emit_internal_runtime_modules(dir: &Path) -> Result<(), GeneratedBuildError> {
    write_file(&dir.join("forge-data-runtime.js"), "export function loadData(){ return Promise.resolve(null); }\n".to_string())?;
    write_file(&dir.join("forge-cache-runtime.js"), "export async function cache(_k, loader){ return await loader(); }\n".to_string())?;
    write_file(&dir.join("forge-navigation-runtime.js"), "export function notFound(){ const e = new Error('Not found'); e.status = 404; return e; }\n".to_string())?;
    Ok(())
}

pub fn relative_import_specifier(from_dir: &Path, to_file: &Path) -> String {
    let relative = pathdiff::diff_paths(to_file, from_dir)
        .unwrap_or_else(|| to_file.to_path_buf());

    let mut value = relative.to_string_lossy().replace('\\', "/");

    if !value.starts_with('.') {
        value = format!("./{value}");
    }

    value
}

pub fn safe_entry_name(route: &str) -> String {
    if route == "/" {
        return "home".to_string();
    }

    route
        .trim_matches('/')
        .replace("[", "")
        .replace("]", "")
        .replace("{", "")
        .replace("}", "")
        .replace('/', "_")
        .replace('-', "_")
}

pub fn source_has_loader_export(source: &str) -> bool {
    source.contains("export async function loader")
        || source.contains("export function loader")
        || source.contains("export const loader")
}

fn create_dir(path: &Path) -> Result<(), GeneratedBuildError> {
    std::fs::create_dir_all(path).map_err(|source| GeneratedBuildError::CreateDir {
        path: path.to_path_buf(),
        source,
    })
}

fn write_file(path: &Path, contents: String) -> Result<(), GeneratedBuildError> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }

    std::fs::write(path, contents).map_err(|source| GeneratedBuildError::WriteFile {
        path: path.to_path_buf(),
        source,
    })
}

fn read_file(path: &Path) -> Result<String, GeneratedBuildError> {
    std::fs::read_to_string(path).map_err(|source| GeneratedBuildError::ReadFile {
        path: path.to_path_buf(),
        source,
    })
}
