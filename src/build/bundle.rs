//! In-process Rolldown. One [`ForgePlugin`] per build handles everything Forge owns:
//! `forge:*` virtual modules, the import policy, CSS collection, asset imports, and
//! the security scan (run in Rolldown's parallel transform phase, app code only).

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow, bail};
use base64::Engine as _;
use rolldown::{
    Bundler, BundlerOptions, InputItem, ModuleType, OutputFormat, Platform, RawMinifyOptions, SourceMapType, TsConfig,
    plugin::{
        HookLoadArgs, HookLoadOutput, HookLoadReturn, HookResolveIdArgs, HookResolveIdOutput, HookResolveIdReturn,
        HookTransformArgs, HookTransformReturn, HookUsage, Plugin, PluginContext, SharedLoadPluginContext,
        SharedTransformPluginContext,
    },
};
use rolldown_common::{Output, side_effects::HookSideEffects};

use super::analyze::{Policy, Violation, analyze, is_internal_specifier};

pub const STATIC_PREFIX: &str = "/_forge/static/";

/// Imported as a URL (or a data URL in single-file builds).
const ASSET_EXTS: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("svg", "image/svg+xml"),
    ("ico", "image/x-icon"),
    ("bmp", "image/bmp"),
    ("mp3", "audio/mpeg"),
    ("ogg", "audio/ogg"),
    ("wav", "audio/wav"),
    ("m4a", "audio/mp4"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
    ("glb", "model/gltf-binary"),
    ("gltf", "model/gltf+json"),
    ("bin", "application/octet-stream"),
    ("hdr", "image/vnd.radiance"),
    ("exr", "image/x-exr"),
    ("ktx2", "image/ktx2"),
    ("wasm", "application/wasm"),
    ("onnx", "application/octet-stream"),
    ("gguf", "application/octet-stream"),
    ("safetensors", "application/octet-stream"),
];

/// Imported as a string: shaders, prompts, data.
const TEXT_EXTS: &[&str] = &["glsl", "vert", "frag", "wgsl", "txt", "md", "csv", "prompt"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Runs in the embedded engine.
    Server,
    /// Runs in the browser.
    Client,
}

#[derive(Debug, Clone)]
pub struct BundleSpec {
    pub root: PathBuf,
    pub inputs: Vec<(String, PathBuf)>,
    pub side: Side,
    pub minify: bool,
    pub sourcemap: bool,
    /// Split dynamic imports into chunks. Off = one file.
    pub split: bool,
    /// Asset imports become data URLs instead of files.
    pub inline_assets: bool,
    pub define: Vec<(String, String)>,
    /// `forge:*` specifier → absolute file.
    pub aliases: Vec<(String, PathBuf)>,
    /// Scan app modules with this policy (do it in one build only).
    pub scan: Option<Policy>,
    /// Modules under this directory are Forge's own and are never scanned.
    pub generated_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub name: String,
    pub file: String,
    pub code: String,
    pub map: Option<String>,
    pub is_entry: bool,
    /// The module this chunk was created for (entry or dynamic-import target).
    pub facade: Option<String>,
    pub imports: Vec<String>,
    pub modules: Vec<(String, usize)>,
}

#[derive(Debug, Default)]
pub struct Bundle {
    pub chunks: Vec<Chunk>,
    /// Every imported stylesheet, in import order.
    pub css: String,
    /// Extra output files (`assets/…`): `(relative path, bytes)`.
    pub files: Vec<(String, Vec<u8>)>,
    pub warnings: Vec<String>,
}

impl Bundle {
    pub fn entry(&self, name: &str) -> Option<&Chunk> {
        self.chunks.iter().find(|c| c.is_entry && c.name == name)
    }

    /// `chunk` plus everything it statically imports, transitively (file names, deduplicated).
    pub fn static_closure(&self, chunk: &Chunk) -> Vec<String> {
        let by_file: HashMap<&str, &Chunk> = self.chunks.iter().map(|c| (c.file.as_str(), c)).collect();
        let mut out = vec![chunk.file.clone()];
        let mut i = 0;
        while i < out.len() {
            if let Some(c) = by_file.get(out[i].as_str()) {
                for import in &c.imports {
                    if !out.contains(import) {
                        out.push(import.clone());
                    }
                }
            }
            i += 1;
        }
        out
    }
}

pub async fn bundle(spec: BundleSpec) -> Result<Bundle> {
    let state = Arc::new(Mutex::new(PluginState::default()));
    let plugin = ForgePlugin {
        aliases: spec.aliases.iter().map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned())).collect(),
        side: spec.side,
        inline_assets: spec.inline_assets,
        scan: spec.scan,
        root: spec.root.to_string_lossy().into_owned(),
        generated_dir: spec.generated_dir.to_string_lossy().into_owned(),
        state: state.clone(),
    };

    let options = BundlerOptions {
        input: Some(
            spec.inputs
                .iter()
                .map(|(name, path)| InputItem { name: Some(name.clone()), import: path.to_string_lossy().into_owned() })
                .collect(),
        ),
        cwd: Some(spec.root.clone()),
        platform: Some(Platform::Browser),
        format: Some(OutputFormat::Esm),
        entry_filenames: Some(
            if spec.side == Side::Server { "[name].js".to_string() } else { "[name].[hash].js".to_string() }.into(),
        ),
        chunk_filenames: Some("chunks/[name].[hash].js".to_string().into()),
        minify: Some(RawMinifyOptions::Bool(spec.minify)),
        sourcemap: spec.sourcemap.then_some(SourceMapType::File),
        define: Some(spec.define.iter().cloned().collect()),
        tsconfig: Some(TsConfig::Auto(true)),
        code_splitting: Some(rolldown::CodeSplittingMode::Bool(spec.split)),
        ..Default::default()
    };

    let mut bundler = Bundler::with_plugins(options, vec![<ForgePlugin as Plugin>::new_shared(plugin)])
        .map_err(|e| anyhow!("{e}"))?;
    let result = bundler.generate().await;
    let _ = bundler.close().await;
    let mut state = std::mem::take(&mut *state.lock().unwrap());

    if !state.violations.is_empty() {
        state.violations.sort_by(|a, b| (&a.file, a.line, a.column).cmp(&(&b.file, b.line, b.column)));
        let list: Vec<String> = state.violations.iter().map(|v| format!("  {v}")).collect();
        bail!("security policy violations:\n{}", list.join("\n"));
    }
    let output = result.map_err(|e| {
        anyhow!("{}", e.iter().map(|d| clean_diagnostic(&d.to_string())).collect::<Vec<_>>().join("\n"))
    })?;

    let mut out = Bundle { warnings: output.warnings.iter().map(|w| w.to_string()).collect(), ..Default::default() };
    let mut maps: HashMap<String, String> = HashMap::new();
    for item in output.assets {
        match item {
            Output::Chunk(c) => {
                let file = c.filename.to_string();
                out.chunks.push(Chunk {
                    name: c.name.to_string(),
                    map: c.map.as_ref().map(|m| m.to_json_string()),
                    code: c.code.clone(),
                    is_entry: c.is_entry,
                    facade: c.facade_module_id.as_ref().map(|m| m.to_string()),
                    imports: c.imports.iter().map(|s| s.to_string()).collect(),
                    modules: c
                        .modules
                        .keys
                        .iter()
                        .zip(&c.modules.values)
                        .map(|(k, v)| (k.to_string(), v.rendered_byte_length()))
                        .collect(),
                    file,
                });
            }
            Output::Asset(a) => {
                let name = a.filename.to_string();
                if name.ends_with(".map") {
                    maps.insert(name, String::from_utf8_lossy(a.source.as_bytes()).into_owned());
                } else {
                    out.files.push((name, a.source.as_bytes().to_vec()));
                }
            }
        }
    }
    for chunk in &mut out.chunks {
        if let Some(map) = maps.remove(&format!("{}.map", chunk.file)) {
            chunk.map = Some(map);
        }
    }

    // Stylesheets in the order their modules execute, entry chunk first.
    let mut seen = std::collections::HashSet::new();
    for chunk in &out.chunks {
        for (id, _) in &chunk.modules {
            if let Some(css) = state.css.get(id) {
                if seen.insert(id.clone()) {
                    out.css.push_str(&format!("/* {} */\n{}\n", relative(&spec.root, id), css.trim_end()));
                }
            }
        }
    }
    out.files.extend(std::mem::take(&mut state.assets));
    Ok(out)
}

/// Rolldown reports plugin errors as internal bugs with a debug dump; keep only our message.
fn clean_diagnostic(msg: &str) -> String {
    let msg = msg.split("\n\nStack backtrace:").next().unwrap_or(msg);
    match msg.strip_prefix("Something went wrong inside rolldown, please report this problem at https://github.com/rolldown/rolldown/issues.") {
        Some(rest) => match rest.find("Caused by:") {
            Some(i) => rest[i + 10..].lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"),
            None => rest.trim().to_string(),
        },
        None => msg.trim().to_string(),
    }
}

fn relative(root: &Path, id: &str) -> String {
    Path::new(id).strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| id.to_string())
}

pub fn content_hash(bytes: &[u8]) -> String {
    let hash = blake3::hash(bytes);
    hash.to_hex()[..10].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_errors_are_unwrapped() {
        let raw = "Something went wrong inside rolldown, please report this problem at https://github.com/rolldown/rolldown/issues.\nplugin `forge` threw an error\n\nCaused by:\n    remote import `https://x` (in app/page.tsx) is denied\n\nStack backtrace:\n   0: foo";
        assert_eq!(clean_diagnostic(raw), "remote import `https://x` (in app/page.tsx) is denied");
        assert_eq!(clean_diagnostic("  [UNRESOLVED_IMPORT] x  "), "[UNRESOLVED_IMPORT] x");
    }
}

#[derive(Debug, Default)]
struct PluginState {
    css: HashMap<String, String>,
    assets: BTreeMap<String, Vec<u8>>,
    violations: Vec<Violation>,
}

#[derive(Debug)]
struct ForgePlugin {
    aliases: HashMap<String, String>,
    side: Side,
    inline_assets: bool,
    scan: Option<Policy>,
    root: String,
    generated_dir: String,
    state: Arc<Mutex<PluginState>>,
}

impl ForgePlugin {
    fn is_app_module(&self, id: &str) -> bool {
        id.starts_with(&self.root) && !id.contains("/node_modules/") && !id.starts_with(&self.generated_dir)
    }
}

fn extension(id: &str) -> &str {
    Path::new(id).extension().and_then(|e| e.to_str()).unwrap_or("")
}

impl Plugin for ForgePlugin {
    fn name(&self) -> Cow<'static, str> {
        "forge".into()
    }

    fn register_hook_usage(&self) -> HookUsage {
        HookUsage::ResolveId | HookUsage::Load | HookUsage::Transform
    }

    async fn resolve_id(&self, _ctx: &PluginContext, args: &HookResolveIdArgs<'_>) -> HookResolveIdReturn {
        let spec = args.specifier;
        if let Some(path) = self.aliases.get(spec) {
            return Ok(Some(HookResolveIdOutput::from_id(path.as_str())));
        }
        // Forge's own entries and runtime may reach internals; nothing else may.
        let Some(importer) = args.importer.filter(|i| !i.starts_with(&self.generated_dir)) else { return Ok(None) };
        let from = relative(Path::new(&self.root), importer);
        if spec.starts_with("forge:") {
            let mut known: Vec<&str> = self.aliases.keys().map(String::as_str).collect();
            known.sort_unstable();
            if is_internal_specifier(spec) {
                bail!("`{spec}` (imported by {from}) is internal to Forge");
            }
            bail!("unknown module `{spec}` (imported by {from}); available: {}", known.join(", "));
        }
        if is_internal_specifier(spec) {
            bail!("`{spec}` (imported by {from}) is internal to Forge");
        }
        if ["http:", "https:", "data:", "blob:"].iter().any(|p| spec.starts_with(p)) {
            bail!("remote import `{spec}` (in {from}) is denied: install the package so it is bundled");
        }
        if spec.starts_with("node:") && self.side == Side::Client {
            bail!("`{spec}` (in {from}) is a Node.js built-in and does not exist in the browser");
        }
        Ok(None)
    }

    async fn load(&self, _ctx: SharedLoadPluginContext, args: &HookLoadArgs<'_>) -> HookLoadReturn {
        let id = args.id;
        let ext = extension(id).to_ascii_lowercase();
        if ext == "css" {
            if self.side == Side::Client {
                let css = std::fs::read_to_string(id)?;
                self.state.lock().unwrap().css.insert(id.to_string(), css);
            }
            // Kept as a side-effect module so its position in the module order survives tree-shaking.
            return Ok(Some(HookLoadOutput {
                code: "".into(),
                module_type: Some(ModuleType::Js),
                side_effects: Some(HookSideEffects::NoTreeshake),
                ..Default::default()
            }));
        }
        if TEXT_EXTS.contains(&ext.as_str()) {
            let text = std::fs::read_to_string(id)?;
            let code = format!("export default {};", serde_json::to_string(&text)?);
            return Ok(Some(HookLoadOutput {
                code: code.into(),
                module_type: Some(ModuleType::Js),
                ..Default::default()
            }));
        }
        if let Some((_, mime)) = ASSET_EXTS.iter().find(|(e, _)| *e == ext) {
            let bytes = std::fs::read(id)?;
            let url = if self.inline_assets {
                format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes))
            } else {
                let stem = Path::new(id).file_stem().and_then(|s| s.to_str()).unwrap_or("asset");
                let stem: String = stem
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                    .collect();
                let name = format!("assets/{stem}.{}.{ext}", content_hash(&bytes));
                let url = format!("{STATIC_PREFIX}{name}");
                if self.side == Side::Client {
                    self.state.lock().unwrap().assets.insert(name, bytes);
                }
                url
            };
            let code = format!("export default {};", serde_json::to_string(&url)?);
            return Ok(Some(HookLoadOutput {
                code: code.into(),
                module_type: Some(ModuleType::Js),
                ..Default::default()
            }));
        }
        Ok(None)
    }

    async fn transform(&self, _ctx: SharedTransformPluginContext, args: &HookTransformArgs<'_>) -> HookTransformReturn {
        let Some(policy) = self.scan else { return Ok(None) };
        if !self.is_app_module(args.id)
            || !matches!(extension(args.id), "js" | "jsx" | "ts" | "tsx" | "mjs" | "mts" | "cjs" | "cts")
        {
            return Ok(None);
        }
        let file = relative(Path::new(&self.root), args.id);
        let analysis = analyze(&file, args.code, policy)?;
        if !analysis.violations.is_empty() {
            self.state.lock().unwrap().violations.extend(analysis.violations);
        }
        Ok(None)
    }
}
