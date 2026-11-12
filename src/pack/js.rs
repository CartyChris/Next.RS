//! JavaScript for packed pages: module scripts bundled into one module (import maps, CDN
//! URLs and bare imports all resolve to npm packages or fetched files), `text/babel` JSX
//! compiled ahead of time, and asset URLs in page code embedded as data URLs.

use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow, bail};
use oxc::{
    allocator::Allocator,
    ast::ast::*,
    ast_visit::{Visit, walk},
    codegen::Codegen,
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
    transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer},
};
use rolldown::{
    Bundler, BundlerOptions, InputItem, ModuleType, OutputFormat, Platform, RawMinifyOptions,
    plugin::{
        HookLoadArgs, HookLoadOutput, HookLoadReturn, HookResolveIdArgs, HookResolveIdOutput, HookResolveIdReturn,
        HookTransformArgs, HookTransformOutput, HookTransformReturn, HookUsage, Plugin, PluginContext,
        SharedLoadPluginContext, SharedTransformPluginContext,
    },
};
use rolldown_common::{Output, side_effects::HookSideEffects};
use url::Url;

use super::{Packer, cdn};
use crate::build::bundle::{ASSET_EXTS, TEXT_EXTS};

// ---- Import maps ----

#[derive(Debug, Default, Clone)]
pub struct ImportMap {
    imports: Vec<(String, String)>,
    /// `(scope URL prefix, map)`, longest scope first.
    scopes: Vec<(String, Vec<(String, String)>)>,
}

fn url_like(s: &str) -> bool {
    s.starts_with('/') || s.starts_with("./") || s.starts_with("../") || Url::parse(s).is_ok()
}

fn absolute(s: &str, base: &Url) -> Option<String> {
    if s.starts_with("//") {
        return Url::parse(&format!("https:{s}")).ok().map(String::from);
    }
    base.join(s).ok().map(String::from)
}

impl ImportMap {
    pub fn parse(json: &str, base: &Url) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(json).context("invalid import map JSON")?;
        let entries = |v: Option<&serde_json::Value>| -> Vec<(String, String)> {
            let mut out: Vec<(String, String)> = v
                .and_then(|v| v.as_object())
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| {
                    let key = if url_like(k) { absolute(k, base)? } else { k.clone() };
                    Some((key, absolute(v.as_str()?, base)?))
                })
                .collect();
            out.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
            out
        };
        let mut scopes: Vec<(String, Vec<(String, String)>)> = value
            .get("scopes")
            .and_then(|s| s.as_object())
            .into_iter()
            .flatten()
            .filter_map(|(scope, map)| Some((absolute(scope, base)?, entries(Some(map)))))
            .collect();
        scopes.sort_by_key(|a| std::cmp::Reverse(a.0.len()));
        Ok(Self { imports: entries(value.get("imports")), scopes })
    }

    fn lookup(map: &[(String, String)], spec: &str) -> Option<String> {
        for (key, value) in map {
            if key == spec {
                return Some(value.clone());
            }
            if key.ends_with('/') && spec.starts_with(key.as_str()) {
                return Some(format!("{value}{}", &spec[key.len()..]));
            }
        }
        None
    }

    /// The URL `spec` maps to when imported from `importer` (a URL), if the map has it.
    pub fn resolve(&self, spec: &str, importer: &Url) -> Option<String> {
        let normalized = if url_like(spec) { absolute(spec, importer)? } else { spec.to_string() };
        for (scope, map) in &self.scopes {
            if importer.as_str().starts_with(scope.as_str()) {
                if let Some(v) = Self::lookup(map, &normalized) {
                    return Some(v);
                }
            }
        }
        Self::lookup(&self.imports, &normalized)
    }
}

// ---- JSX ahead of time ----

/// Compile a `text/babel` script: JSX with the classic runtime (`React.createElement`, the
/// global from a UMD build), TypeScript stripped when asked. No module wrapper, so top-level
/// declarations stay global, as when Babel runs them in the page.
pub fn compile_jsx(code: &str, typescript: bool, module: bool) -> Result<String> {
    let allocator = Allocator::default();
    let source_type = if typescript { SourceType::tsx() } else { SourceType::jsx() };
    let source_type = if module { source_type.with_module(true) } else { source_type.with_script(true) };
    let parsed = Parser::new(&allocator, code, source_type).parse();
    if let Some(e) = parsed.diagnostics.errors().next() {
        bail!("text/babel script: {e}");
    }
    let mut program = parsed.program;
    let scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();
    let options = TransformOptions {
        jsx: JsxOptions { runtime: JsxRuntime::Classic, display_name_plugin: false, ..JsxOptions::enable() },
        ..TransformOptions::default()
    };
    let path = if typescript { "inline.tsx" } else { "inline.jsx" };
    let result = Transformer::new(&allocator, Path::new(path), &options).build_with_scoping(scoping, &mut program);
    if let Some(e) = result.diagnostics.errors().next() {
        bail!("text/babel script: {e}");
    }
    Ok(Codegen::new().build(&program).code)
}

// ---- Asset URLs in code ----

/// Extensions that mark a string as a file the code loads at runtime.
fn asset_like(s: &str) -> bool {
    if s.len() > 2048 || s.is_empty() || s.contains(char::is_whitespace) || s.contains("${") {
        return false;
    }
    if ["data:", "blob:", "#", "javascript:", "mailto:"].iter().any(|p| s.starts_with(p)) {
        return false;
    }
    let path = s.split(['?', '#']).next().unwrap_or(s);
    let Some((_, ext)) = path.rsplit_once('.') else { return false };
    let ext = ext.to_ascii_lowercase();
    ASSET_EXTS.iter().any(|(e, _)| *e == ext)
        || TEXT_EXTS.contains(&ext.as_str())
        || matches!(ext.as_str(), "json" | "obj" | "mtl" | "fbx" | "stl" | "ply" | "atlas" | "fnt")
}

struct Literal {
    start: usize,
    end: usize,
    value: String,
    /// `new URL(value, import.meta.url)`: relative to the module, not the page.
    module_relative: bool,
}

#[derive(Default)]
struct Literals(Vec<Literal>);

impl<'a> Visit<'a> for Literals {
    fn visit_string_literal(&mut self, it: &StringLiteral<'a>) {
        if let Some(v) = it.value.as_str().filter(|v| asset_like(v)) {
            self.0.push(Literal {
                start: it.span.start as usize,
                end: it.span.end as usize,
                value: v.to_string(),
                module_relative: false,
            });
        }
    }
    fn visit_template_literal(&mut self, it: &TemplateLiteral<'a>) {
        if it.expressions.is_empty() {
            if let Some(cooked) = it.quasis.first().and_then(|q| q.value.cooked).and_then(|c| c.as_str()) {
                if asset_like(cooked) {
                    self.0.push(Literal {
                        start: it.span.start as usize,
                        end: it.span.end as usize,
                        value: cooked.to_string(),
                        module_relative: false,
                    });
                }
            }
        }
        walk::walk_template_literal(self, it);
    }
    fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
        let is_url = matches!(it.callee.without_parentheses(), Expression::Identifier(id) if id.name == "URL");
        let meta = it.arguments.get(1).and_then(|a| a.as_expression()).is_some_and(|e| {
            matches!(e, Expression::StaticMemberExpression(m) if m.property.name == "url" && matches!(m.object.without_parentheses(), Expression::ImportMeta(_)))
        });
        if let (true, true, Some(Expression::StringLiteral(s))) =
            (is_url, meta, it.arguments.first().and_then(|a| a.as_expression()))
        {
            if let Some(v) = s.value.as_str().filter(|v| asset_like(v)) {
                self.0.push(Literal {
                    start: it.span.start as usize,
                    end: it.span.end as usize,
                    value: v.to_string(),
                    module_relative: true,
                });
                return;
            }
        }
        walk::walk_new_expression(self, it);
    }
    // Specifiers, keys and tagged templates are not runtime URLs.
    fn visit_import_declaration(&mut self, _: &ImportDeclaration<'a>) {}
    fn visit_export_all_declaration(&mut self, _: &ExportAllDeclaration<'a>) {}
    fn visit_export_named_declaration(&mut self, _: &ExportNamedDeclaration<'a>) {}
    fn visit_export_from_declaration(&mut self, _: &ExportFromDeclaration<'a>) {}
    fn visit_import_expression(&mut self, _: &ImportExpression<'a>) {}
    fn visit_property_key(&mut self, it: &PropertyKey<'a>) {
        if let PropertyKey::StringLiteral(_) = it {
            return;
        }
        walk::walk_property_key(self, it);
    }
    fn visit_tagged_template_expression(&mut self, it: &TaggedTemplateExpression<'a>) {
        self.visit_expression(&it.tag);
    }
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if matches!(it.callee.without_parentheses(), Expression::Identifier(id) if id.name == "require") {
            return;
        }
        walk::walk_call_expression(self, it);
    }
}

/// Asset-like literals in `code` (none if it does not parse).
fn literals(code: &str, source_type: SourceType) -> Literals {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    let mut found = Literals::default();
    if !parsed.fatal_error && !parsed.diagnostics.has_errors() {
        found.visit_program(&parsed.program);
    }
    found
}

impl Packer {
    /// Embed the files page code loads at runtime: string literals naming a local file next
    /// to the page (resolved like the browser does, against the page), or an absolute URL
    /// with an asset extension. `module_base` is the module's own URL for `new URL(x,
    /// import.meta.url)`. Returns the new code, or `None` when nothing changed.
    pub async fn sweep(
        &self,
        code: &str,
        source_type: SourceType,
        page_base: &Url,
        module_base: &Url,
    ) -> Result<Option<String>> {
        let found = literals(code, source_type);
        if found.0.is_empty() {
            return Ok(None);
        }
        let loads = found.0.iter().map(|lit| async move {
            let base = if lit.module_relative { module_base } else { page_base };
            let url = absolute(&lit.value, base)?;
            let url = Url::parse(&url).ok()?;
            let remote = matches!(url.scheme(), "http" | "https");
            // A relative name only counts when the file is really there.
            if url.scheme() == "file" && self.local_path(&url).is_err() {
                if !lit.value.contains("://") {
                    self.note(format!("kept \"{}\" in code: no such file next to the page", lit.value));
                }
                return None;
            }
            if !remote && url.scheme() != "file" {
                return None;
            }
            match self.load(&url).await {
                Ok(res) => Some(res.data_url()),
                Err(e) => {
                    self.warn(format!("kept \"{}\" in code: {e:#}", lit.value));
                    None
                }
            }
        });
        let urls = futures_util::future::join_all(loads).await;
        let mut out = String::with_capacity(code.len());
        let mut last = 0;
        let mut changed = false;
        for (lit, data) in found.0.iter().zip(urls) {
            let Some(data) = data else { continue };
            if lit.start < last {
                continue;
            }
            out.push_str(&code[last..lit.start]);
            let quoted = serde_json::to_string(&data)?;
            if lit.module_relative {
                out.push_str(&format!("new URL({quoted})"));
            } else {
                out.push_str(&quoted);
            }
            last = lit.end;
            changed = true;
        }
        out.push_str(&code[last..]);
        Ok(changed.then_some(out))
    }
}

// ---- Module bundling ----

pub enum ModuleInput {
    /// An inline `<script type="module">`.
    Inline(String),
    /// `<script type="module" src>`.
    Url(Url),
}

pub struct Bundled {
    pub js: String,
    pub css: String,
}

const VIRTUAL: &str = "__forge_pack_";

struct PackPlugin {
    p: Arc<Packer>,
    virtuals: HashMap<String, String>,
    map: Option<ImportMap>,
    base: Url,
    css: Arc<Mutex<HashMap<String, (String, Url)>>>,
}

fn package_name(spec: &str) -> &str {
    let mut parts = spec.splitn(3, '/');
    match (spec.starts_with('@'), parts.next(), parts.next()) {
        (true, Some(scope), Some(name)) => &spec[..scope.len() + 1 + name.len()],
        (_, Some(name), _) => name,
        _ => spec,
    }
}

fn is_bare(spec: &str) -> bool {
    !(spec.starts_with('.')
        || spec.starts_with('/')
        || spec.starts_with('\0')
        || Url::parse(spec).is_ok()
        || Path::new(spec).is_absolute())
}

impl PackPlugin {
    fn importer_url(&self, importer: Option<&str>) -> Url {
        match importer {
            Some(i) if i.starts_with("http://") || i.starts_with("https://") => {
                Url::parse(i).unwrap_or(self.base.clone())
            }
            Some(i) if self.virtuals.contains_key(i) => self.base.clone(),
            Some(i) => Url::from_file_path(i).unwrap_or(self.base.clone()),
            None => self.base.clone(),
        }
    }

    fn in_workspace(&self, id: &str) -> bool {
        Path::new(id).starts_with(self.p.npm.workspace())
    }

    /// Code the page's author wrote (swept for asset URLs), as opposed to packages.
    fn is_page_code(&self, id: &str) -> bool {
        self.virtuals.contains_key(id) || (Path::new(id).starts_with(&self.p.dir) && !id.contains("/node_modules/"))
    }

    /// Resolve `spec` as if imported from inside the package directory `pkg_dir`'s tree.
    async fn resolve_from(
        &self,
        ctx: &PluginContext,
        spec: &str,
        pkg_dir: &Path,
        name: &str,
    ) -> Result<HookResolveIdOutput> {
        let mut root = pkg_dir.to_path_buf();
        for _ in 0..name.split('/').count() + 1 {
            root.pop();
        }
        let importer = root.join("__forge_importer.js");
        match ctx.resolve(spec, Some(&importer.to_string_lossy()), None).await? {
            Ok(r) => Ok(HookResolveIdOutput::from_resolved_id(r)),
            Err(e) => bail!("cannot resolve `{spec}` in the installed package `{name}`: {e}"),
        }
    }

    async fn resolve_url(&self, ctx: &PluginContext, url: Url) -> Result<HookResolveIdOutput> {
        match cdn::parse(&url) {
            Some(r @ cdn::CdnRef::File { .. }) => {
                Ok(HookResolveIdOutput::from_id(self.p.cdn_file(&r).await?.to_string_lossy().into_owned()))
            }
            Some(r) => {
                let placed = self.p.npm.root(r.name(), r.range()).await?;
                self.p.source(&url, format!("npm {}", r.name()));
                self.resolve_from(ctx, &cdn::specifier(&r), &placed.dir, r.name()).await
            }
            None if url.scheme() == "file" => {
                Ok(HookResolveIdOutput::from_id(self.p.local_path(&url)?.to_string_lossy().into_owned()))
            }
            None => Ok(HookResolveIdOutput::from_id(url.to_string())),
        }
    }
}

impl std::fmt::Debug for PackPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PackPlugin")
    }
}

impl Plugin for PackPlugin {
    fn name(&self) -> Cow<'static, str> {
        "forge-pack".into()
    }

    fn register_hook_usage(&self) -> HookUsage {
        HookUsage::ResolveId | HookUsage::Load | HookUsage::Transform
    }

    async fn resolve_id(&self, ctx: &PluginContext, args: &HookResolveIdArgs<'_>) -> HookResolveIdReturn {
        let spec = args.specifier;
        if self.virtuals.contains_key(spec) {
            return Ok(Some(HookResolveIdOutput::from_id(spec)));
        }
        let importer_url = self.importer_url(args.importer);
        let mapped = self.map.as_ref().and_then(|m| m.resolve(spec, &importer_url));
        let spec = mapped.as_deref().unwrap_or(spec);
        let remote_importer = matches!(importer_url.scheme(), "http" | "https");
        if spec.starts_with("//") || ["http://", "https://", "file://"].iter().any(|s| spec.starts_with(s)) {
            let url = Url::parse(&absolute(spec, &importer_url).ok_or_else(|| anyhow!("bad URL `{spec}`"))?)?;
            if !matches!(url.scheme(), "http" | "https" | "file") {
                bail!("cannot import `{spec}`");
            }
            return self.resolve_url(ctx, url).await.map(Some);
        }
        if remote_importer && !is_bare(spec) {
            let url = importer_url.join(spec)?;
            return self.resolve_url(ctx, url).await.map(Some);
        }
        if is_bare(spec) {
            if spec.starts_with("node:") {
                bail!("`{spec}` is a Node.js built-in and does not exist in the browser");
            }
            let local = args.importer.is_some_and(|i| !i.starts_with("http") && !self.virtuals.contains_key(i));
            if local && self.in_workspace(args.importer.unwrap_or_default()) {
                return Ok(None);
            }
            // The page's own node_modules first (component projects), then the workspace.
            if let Some(importer) = args.importer.filter(|_| local) {
                if let Ok(Ok(r)) = ctx.resolve(spec, Some(importer), None).await {
                    return Ok(Some(HookResolveIdOutput::from_resolved_id(r)));
                }
            }
            let name = package_name(spec);
            let placed = self.p.npm.root(name, &self.p.range_for(name)).await?;
            return self.resolve_from(ctx, spec, &placed.dir, name).await.map(Some);
        }
        if args.importer.is_some_and(|i| self.virtuals.contains_key(i)) {
            // Relative imports in inline scripts resolve against the page.
            let url = self.base.join(spec)?;
            return self.resolve_url(ctx, url).await.map(Some);
        }
        Ok(None)
    }

    async fn load(&self, _ctx: SharedLoadPluginContext, args: &HookLoadArgs<'_>) -> HookLoadReturn {
        let id = args.id;
        if let Some(code) = self.virtuals.get(id) {
            return Ok(Some(HookLoadOutput {
                code: code.clone().into(),
                module_type: Some(ModuleType::Js),
                ..Default::default()
            }));
        }
        let remote = id.starts_with("http://") || id.starts_with("https://");
        let ext = {
            let p =
                if remote { Url::parse(id).map(|u| u.path().to_string()).unwrap_or_default() } else { id.to_string() };
            Path::new(&p).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
        };
        if ext == "css" {
            let url =
                if remote { Url::parse(id)? } else { Url::from_file_path(id).map_err(|_| anyhow!("bad path {id}"))? };
            let res = self.p.load(&url).await?;
            self.css.lock().unwrap().insert(id.to_string(), (res.text(), res.url.clone()));
            return Ok(Some(HookLoadOutput {
                code: "".into(),
                module_type: Some(ModuleType::Js),
                side_effects: Some(HookSideEffects::NoTreeshake),
                ..Default::default()
            }));
        }
        let asset = ASSET_EXTS.iter().any(|(e, _)| *e == ext);
        let text = TEXT_EXTS.contains(&ext.as_str());
        if remote || asset || text {
            let url =
                if remote { Url::parse(id)? } else { Url::from_file_path(id).map_err(|_| anyhow!("bad path {id}"))? };
            let res = self.p.load(&url).await?;
            let code = if asset {
                format!("export default {};", serde_json::to_string(&res.data_url())?)
            } else if text {
                format!("export default {};", serde_json::to_string(&res.text())?)
            } else if ext == "json" || res.mime == "application/json" {
                format!("export default {};", res.text())
            } else {
                res.text()
            };
            return Ok(Some(HookLoadOutput {
                code: code.into(),
                module_type: Some(ModuleType::Js),
                ..Default::default()
            }));
        }
        Ok(None)
    }

    async fn transform(&self, _ctx: SharedTransformPluginContext, args: &HookTransformArgs<'_>) -> HookTransformReturn {
        if !self.is_page_code(args.id) {
            return Ok(None);
        }
        let module_base = if self.virtuals.contains_key(args.id) {
            self.base.clone()
        } else {
            Url::from_file_path(args.id).unwrap_or(self.base.clone())
        };
        let source_type = SourceType::from_path(Path::new(args.id)).unwrap_or_else(|_| SourceType::mjs());
        Ok(self
            .p
            .sweep(args.code, source_type, &self.base, &module_base)
            .await?
            .map(|code| HookTransformOutput { code: Some(code), ..Default::default() }))
    }
}

impl Packer {
    /// Bundle a page's module scripts (in document order) into one ES module.
    pub async fn bundle_modules(
        self: &Arc<Self>,
        inputs: Vec<ModuleInput>,
        map: Option<ImportMap>,
        base: &Url,
    ) -> Result<Bundled> {
        let mut virtuals = HashMap::new();
        let mut entry = String::new();
        let dir = self.dir.to_string_lossy().into_owned();
        for (i, input) in inputs.into_iter().enumerate() {
            match input {
                ModuleInput::Inline(code) => {
                    let id = format!("{dir}/{VIRTUAL}script{i}.js");
                    entry.push_str(&format!("import {};\n", serde_json::to_string(&id)?));
                    virtuals.insert(id, code);
                }
                ModuleInput::Url(url) => entry.push_str(&format!("import {};\n", serde_json::to_string(url.as_str())?)),
            }
        }
        let entry_id = format!("{dir}/{VIRTUAL}entry.js");
        virtuals.insert(entry_id.clone(), entry);
        self.bundle(virtuals, &entry_id, map, base).await
    }

    /// Bundle from `entry` (an id in `virtuals` or a real file).
    pub async fn bundle(
        self: &Arc<Self>,
        virtuals: HashMap<String, String>,
        entry: &str,
        map: Option<ImportMap>,
        base: &Url,
    ) -> Result<Bundled> {
        let css_state: Arc<Mutex<HashMap<String, (String, Url)>>> = Default::default();
        let plugin = PackPlugin { p: self.clone(), virtuals, map, base: base.clone(), css: css_state.clone() };
        let options = BundlerOptions {
            input: Some(vec![InputItem { name: Some("page".into()), import: entry.to_string() }]),
            cwd: Some(self.dir.clone()),
            platform: Some(Platform::Browser),
            format: Some(OutputFormat::Esm),
            minify: Some(RawMinifyOptions::Bool(self.options.minify)),
            define: Some(
                [
                    ("process.env.NODE_ENV".to_string(), "\"production\"".to_string()),
                    ("import.meta.env.MODE".into(), "\"production\"".into()),
                ]
                .into_iter()
                .collect(),
            ),
            code_splitting: Some(rolldown::CodeSplittingMode::Bool(false)),
            ..Default::default()
        };
        let mut bundler = Bundler::with_plugins(options, vec![<PackPlugin as Plugin>::new_shared(plugin)])
            .map_err(|e| anyhow!("{e}"))?;
        let result = bundler.generate().await;
        let _ = bundler.close().await;
        let output =
            result.map_err(|e| anyhow!("{}", e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")))?;
        let unresolved: Vec<String> =
            output.warnings.iter().map(|w| w.to_string()).filter(|w| w.starts_with("Could not resolve")).collect();
        if !unresolved.is_empty() {
            bail!("unresolved imports:\n  {}", unresolved.join("\n  "));
        }
        let mut js = String::new();
        let mut order: Vec<String> = Vec::new();
        for item in output.assets {
            if let Output::Chunk(c) = item {
                if !js.is_empty() {
                    bail!("module scripts split into several chunks");
                }
                js = c.code.clone();
                order = c.modules.keys.iter().map(|k| k.to_string()).collect();
            }
        }
        let collected = std::mem::take(&mut *css_state.lock().unwrap());
        let mut css = String::new();
        for id in order {
            if let Some((text, url)) = collected.get(&id) {
                css.push_str(&self.inline_css(text, url, 0).await?);
                css.push('\n');
            }
        }
        Ok(Bundled { js, css })
    }
}

/// Where a file inside the package workspace came from, for messages.
pub fn short(path: &Path, ws: &Path) -> String {
    path.strip_prefix(ws.join("node_modules"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_maps() {
        let base = Url::parse("file:///site/index.html").unwrap();
        let map = ImportMap::parse(
            r#"{"imports":{"three":"https://unpkg.com/three@0.160.0/build/three.module.js","three/addons/":"https://unpkg.com/three@0.160.0/examples/jsm/","./lib/":"./vendor/"},
                "scopes":{"https://unpkg.com/":{"three":"https://unpkg.com/three@0.150.0/build/three.module.js"}}}"#,
            &base,
        )
        .unwrap();
        assert_eq!(map.resolve("three", &base).unwrap(), "https://unpkg.com/three@0.160.0/build/three.module.js");
        assert_eq!(
            map.resolve("three/addons/controls/OrbitControls.js", &base).unwrap(),
            "https://unpkg.com/three@0.160.0/examples/jsm/controls/OrbitControls.js"
        );
        assert_eq!(map.resolve("./lib/a.js", &base).unwrap(), "file:///site/vendor/a.js");
        let scoped = Url::parse("https://unpkg.com/x/y.js").unwrap();
        assert_eq!(map.resolve("three", &scoped).unwrap(), "https://unpkg.com/three@0.150.0/build/three.module.js");
        assert_eq!(map.resolve("react", &base), None);
    }

    #[test]
    fn babel_scripts_compile_ahead_of_time() {
        let out = compile_jsx(
            "function App() { return <h1 className=\"x\">Hi {name}</h1>; }\nconst y = <><App /></>;",
            false,
            false,
        )
        .unwrap();
        assert!(
            out.contains("React.createElement(\"h1\"")
                && out.contains("React.Fragment")
                && out.contains("function App()"),
            "{out}"
        );
        let ts = compile_jsx("const n: number = 1; const el = <b>{n as number}</b>;", true, false).unwrap();
        assert!(!ts.contains(": number") && ts.contains("React.createElement(\"b\""), "{ts}");
        assert!(compile_jsx("const x = <div>", false, false).is_err());
    }

    #[test]
    fn asset_literals() {
        for yes in [
            "textures/crate.gif",
            "https://threejs.org/examples/models/gltf/Horse.glb",
            "./a.PNG?v=2",
            "level.json",
            "s.frag",
        ] {
            assert!(asset_like(yes), "{yes}");
        }
        for no in ["hello", "a b.png", "data:image/png;base64,x", "${x}.png", "#id", "file.js", "style.css"] {
            assert!(!asset_like(no), "{no}");
        }
        let src = r#"import t from "./t.png"; export * from "./x.png"; export { y } from "./y.png";
const a = "a.png", b = `b.png`, c = String.raw`c.png`, d = { "d.png": 1 }, e = require("e.png");
const f = new URL("./f.png", import.meta.url); load(`g.${n}.png`); export const h = "h.png";"#;
        let found = literals(src, SourceType::mjs());
        let values: Vec<(&str, bool)> = found.0.iter().map(|l| (l.value.as_str(), l.module_relative)).collect();
        assert_eq!(values, [("a.png", false), ("b.png", false), ("./f.png", true), ("h.png", false)]);
    }

    #[test]
    fn bare_specifiers() {
        assert_eq!(package_name("react-dom/client"), "react-dom");
        assert_eq!(package_name("@react-three/fiber"), "@react-three/fiber");
        assert_eq!(package_name("@scope/pkg/sub/path.js"), "@scope/pkg");
        assert!(is_bare("three") && is_bare("@a/b") && !is_bare("./x") && !is_bare("/x") && !is_bare("https://x"));
    }
}
