//! `forge pack`: one offline HTML file from an HTML page or a component file.
//!
//! * [`npm`]: a small npm installer (CDN URLs become registry packages)
//! * [`cdn`]: which CDN URLs are npm packages
//! * [`net`]: HTTP with a disk cache, for everything else
//! * [`css`]: `@import` and `url()` inlining
//! * [`js`]: module scripts bundled with Rolldown, JSX compiled ahead of time, asset URLs in code
//! * [`html`]: the page rewrite
//! * [`inline`]: compressed classic scripts and styles that still run in place
//! * [`component`]: a single component or script file becomes a page first

pub mod cdn;
pub mod component;
pub mod css;
pub mod html;
pub mod inline;
pub mod js;
pub mod net;
pub mod npm;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use url::Url;

use crate::config::Compress;
use cdn::CdnRef;

#[derive(Debug, Clone)]
pub struct Options {
    pub compress: Compress,
    pub minify: bool,
    /// Use only the package and download caches.
    pub offline: bool,
    pub registry: String,
    pub cache_dir: PathBuf,
    /// Component files: add Tailwind's browser build (`None`: when the markup uses it).
    pub tailwind: Option<bool>,
    /// Component files: the page title (default: from the file name).
    pub title: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            compress: Compress::Auto,
            minify: true,
            offline: false,
            registry: default_registry(),
            cache_dir: default_cache_dir(),
            tailwind: None,
            title: None,
        }
    }
}

/// `FORGE_CACHE_DIR`, else `$XDG_CACHE_HOME/forge`, else `~/.cache/forge`.
pub fn default_cache_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("FORGE_CACHE_DIR") {
        return d.into();
    }
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(d).join("forge");
    }
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(h) => PathBuf::from(h).join(".cache").join("forge"),
        None => std::env::temp_dir().join("forge-cache"),
    }
}

/// `FORGE_NPM_REGISTRY`, else npm's `npm_config_registry`, else the public registry.
pub fn default_registry() -> String {
    std::env::var("FORGE_NPM_REGISTRY")
        .or_else(|_| std::env::var("npm_config_registry"))
        .unwrap_or_else(|_| "https://registry.npmjs.org".into())
}

pub struct Resource {
    pub bytes: Vec<u8>,
    pub mime: String,
    /// Where it was loaded from in the end: relative references resolve against this.
    pub url: Url,
}

impl Resource {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(self.bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&self.bytes)).into_owned()
    }
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime, base64::engine::general_purpose::STANDARD.encode(&self.bytes))
    }
}

/// What a pack did, for the command's report.
#[derive(Debug, Default)]
pub struct Summary {
    pub output: PathBuf,
    pub bytes: usize,
    pub input_bytes: usize,
    /// `(what, where it came from)`, sorted.
    pub sources: Vec<(String, String)>,
    pub packages: Vec<String>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct Packer {
    /// The input file as a URL: the page base for relative references.
    pub page: Url,
    pub dir: PathBuf,
    pub options: Options,
    pub http: net::Http,
    pub npm: npm::Installer,
    /// Version ranges from a `package.json` next to the input.
    deps: BTreeMap<String, String>,
    warnings: Mutex<Vec<String>>,
    notes: Mutex<Vec<String>>,
    sources: Mutex<BTreeMap<String, String>>,
    loaded: tokio::sync::Mutex<HashMap<Url, Arc<Resource>>>,
    /// JSX import source for bundled modules (`preact`); React's by default.
    pub jsx_import_source: Mutex<Option<String>>,
}

impl Packer {
    pub fn new(input: &Path, options: Options) -> Result<Arc<Self>> {
        let input = input.canonicalize().with_context(|| format!("{} not found", input.display()))?;
        let dir = input.parent().context("input has no directory")?.to_path_buf();
        let page = Url::from_file_path(&input).map_err(|_| anyhow!("{} is not a file path", input.display()))?;
        let key = blake3::hash(dir.to_string_lossy().as_bytes()).to_hex()[..16].to_string();
        let ws = options.cache_dir.join("work").join(key);
        std::fs::create_dir_all(&ws).with_context(|| format!("creating {}", ws.display()))?;
        let ws = ws.canonicalize()?;
        let client = net::client()?;
        let registry = Arc::new(npm::Registry::new(
            &options.registry,
            options.cache_dir.join("npm"),
            client.clone(),
            options.offline,
        ));
        let http = net::Http::new(client, options.cache_dir.join("http"), options.offline);
        let mut deps = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(dir.join("package.json")) {
            let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            for field in ["devDependencies", "dependencies"] {
                for (k, v) in json.get(field).and_then(|d| d.as_object()).into_iter().flatten() {
                    if let Some(v) = v.as_str() {
                        deps.insert(k.clone(), v.to_string());
                    }
                }
            }
        }
        Ok(Arc::new(Self {
            page,
            dir,
            http,
            npm: npm::Installer::new(registry, ws),
            options,
            deps,
            warnings: Default::default(),
            notes: Default::default(),
            sources: Default::default(),
            loaded: Default::default(),
            jsx_import_source: Default::default(),
        }))
    }

    pub fn warn(&self, msg: impl Into<String>) {
        let msg = msg.into();
        let mut w = self.warnings.lock().unwrap();
        if !w.contains(&msg) {
            w.push(msg);
        }
    }

    pub fn note(&self, msg: impl Into<String>) {
        self.notes.lock().unwrap().push(msg.into());
    }

    pub(crate) fn source(&self, what: &Url, from: String) {
        let what = match what.scheme() {
            "file" => self.display_path(what),
            _ => what.to_string(),
        };
        self.sources.lock().unwrap().insert(what, from);
    }

    fn display_path(&self, url: &Url) -> String {
        let path = url.to_file_path().unwrap_or_default();
        if let Ok(rel) = path.strip_prefix(&self.dir) {
            return rel.to_string_lossy().replace('\\', "/");
        }
        if let Ok(rel) = path.strip_prefix(self.npm.workspace().join("node_modules")) {
            return format!("npm:{}", rel.to_string_lossy().replace('\\', "/"));
        }
        path.display().to_string()
    }

    pub fn summary(&self) -> Summary {
        Summary {
            sources: self.sources.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            packages: self.npm.log.lock().unwrap().clone(),
            notes: self.notes.lock().unwrap().clone(),
            warnings: self.warnings.lock().unwrap().clone(),
            ..Default::default()
        }
    }

    /// The range for a bare import: the input's `package.json`, else the latest version.
    pub fn range_for(&self, name: &str) -> String {
        self.deps.get(name).cloned().unwrap_or_else(|| "latest".into())
    }

    /// A local file the page may embed: inside the input's directory (no hidden files or
    /// folders, nothing outside through symlinks) or in the package workspace.
    pub fn local_path(&self, url: &Url) -> Result<PathBuf> {
        let path = url.to_file_path().map_err(|_| anyhow!("{url} is not a local path"))?;
        let real = path.canonicalize().with_context(|| format!("{} not found", path.display()))?;
        if let Ok(rel) = real.strip_prefix(&self.dir) {
            let hidden =
                rel.components().any(|c| matches!(c, Component::Normal(n) if n.to_string_lossy().starts_with('.')));
            if hidden {
                bail!("{} is hidden; hidden files are never packed", rel.display());
            }
            return Ok(real);
        }
        if real.starts_with(self.npm.workspace()) {
            return Ok(real);
        }
        bail!("{} is outside the page's folder; only files next to the page are packed", path.display())
    }

    /// Fetch a resource: data URLs, local files, CDN URLs (from npm), anything else over HTTP.
    pub async fn load(&self, url: &Url) -> Result<Arc<Resource>> {
        if let Some(r) = self.loaded.lock().await.get(url) {
            return Ok(r.clone());
        }
        let res = match url.scheme() {
            "data" => decode_data_url(url)?,
            "file" => {
                let path = self.local_path(url)?;
                let bytes = std::fs::read(&path)?;
                self.source(url, "local".into());
                Resource { mime: mime(&path.to_string_lossy(), None, &bytes), bytes, url: url.clone() }
            }
            "http" | "https" => match cdn::parse(url) {
                Some(r) => {
                    let path = self.cdn_file(&r).await?;
                    let bytes = std::fs::read(&path)?;
                    self.source(url, format!("npm {}", r.name()));
                    let file = Url::from_file_path(&path).map_err(|_| anyhow!("bad path"))?;
                    Resource { mime: mime(&path.to_string_lossy(), None, &bytes), bytes, url: file }
                }
                None => {
                    let got = self.http.get(url).await?;
                    self.source(url, "http".into());
                    Resource {
                        mime: mime(got.url.path(), got.content_type.as_deref(), &got.bytes),
                        bytes: got.bytes,
                        url: got.url,
                    }
                }
            },
            other => bail!("unsupported URL scheme `{other}:`"),
        };
        let res = Arc::new(res);
        self.loaded.lock().await.insert(url.clone(), res.clone());
        Ok(res)
    }

    /// The file a CDN URL serves, from the npm package. Packages without a file path get the
    /// file the CDN would pick (`unpkg` / `jsdelivr` / `browser` / `main`).
    pub async fn cdn_file(&self, r: &CdnRef) -> Result<PathBuf> {
        let placed = self.npm.root(r.name(), r.range()).await?;
        let path = match r {
            CdnRef::File { path, .. } => path.clone(),
            CdnRef::Package { subpath: Some(s), .. } => s.clone(),
            CdnRef::Package { subpath: None, .. } => {
                let json: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(placed.dir.join("package.json"))?)?;
                ["unpkg", "jsdelivr", "browser", "main"]
                    .iter()
                    .find_map(|f| json.get(*f).and_then(|v| v.as_str()).map(String::from))
                    .unwrap_or_else(|| "index.js".into())
            }
        };
        let mut candidates = vec![path.clone()];
        // jsDelivr minifies on the fly: `x.min.js` may not exist in the package.
        for ext in [".js", ".css", ".mjs"] {
            if let Some(stem) = path.strip_suffix(&format!(".min{ext}")) {
                candidates.push(format!("{stem}{ext}"));
            }
        }
        if Path::new(&path).extension().is_none() {
            candidates.extend([format!("{path}.js"), format!("{path}/index.js")]);
        }
        for c in &candidates {
            let full = placed.dir.join(c.trim_start_matches('/'));
            if full.is_file() && full.starts_with(&placed.dir) {
                return Ok(full);
            }
        }
        bail!("{}@{} has no file `{path}`", placed.name, placed.version)
    }

    /// Check a fetched resource against an `integrity` attribute (SRI).
    pub fn check_integrity(&self, res: &Resource, integrity: &str) -> Result<()> {
        use sha2::Digest as _;
        let b64 = |d: &[u8]| base64::engine::general_purpose::STANDARD.encode(d);
        let mut known = false;
        for item in integrity.split_whitespace() {
            let (alg, want) = item.split_once('-').unwrap_or((item, ""));
            let want = want.split('?').next().unwrap_or("");
            let got = match alg {
                "sha256" => b64(&sha2::Sha256::digest(&res.bytes)),
                "sha384" => b64(&sha2::Sha384::digest(&res.bytes)),
                "sha512" => b64(&sha2::Sha512::digest(&res.bytes)),
                _ => continue,
            };
            known = true;
            if got == want {
                return Ok(());
            }
        }
        if known {
            bail!("{} does not match its integrity attribute", res.url);
        }
        Ok(())
    }
}

fn decode_data_url(url: &Url) -> Result<Resource> {
    let raw = url.as_str().strip_prefix("data:").unwrap_or_default();
    let (head, body) = raw.split_once(',').context("malformed data URL")?;
    let base64 = head.ends_with(";base64");
    let mime = head
        .trim_end_matches(";base64")
        .split(';')
        .next()
        .filter(|m| !m.is_empty())
        .unwrap_or("text/plain")
        .to_string();
    let bytes = if base64 {
        base64::engine::general_purpose::STANDARD
            .decode(percent_encoding::percent_decode_str(body).collect::<Vec<u8>>())?
    } else {
        percent_encoding::percent_decode_str(body).collect()
    };
    Ok(Resource { bytes, mime, url: url.clone() })
}

/// MIME type from the file extension, then the server's header, then the bytes.
pub fn mime(name: &str, header: Option<&str>, bytes: &[u8]) -> String {
    let ext = name.rsplit('/').next().unwrap_or(name).rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let known = match ext.as_deref() {
        Some("css") => Some("text/css"),
        Some("js" | "mjs" | "cjs") => Some("text/javascript"),
        Some("json" | "map") => Some("application/json"),
        Some("html" | "htm") => Some("text/html"),
        Some("txt") => Some("text/plain"),
        Some(e) => crate::build::bundle::ASSET_EXTS.iter().find(|(x, _)| *x == e).map(|(_, m)| *m),
        None => None,
    };
    if let Some(m) = known {
        return m.to_string();
    }
    if let Some(h) = header.and_then(|h| h.split(';').next()).map(str::trim).filter(|h| !h.is_empty()) {
        return h.to_string();
    }
    let sniffed = match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        [b'w', b'O', b'F', b'2', ..] => "font/woff2",
        [b'w', b'O', b'F', b'F', ..] => "font/woff",
        [b'g', b'l', b'T', b'F', ..] => "model/gltf-binary",
        _ if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") => "image/svg+xml",
        _ => "application/octet-stream",
    };
    sniffed.to_string()
}

/// Pack `input` into `output`. An HTML page becomes `<name>.packed.html` by default; a
/// component or script file becomes `<name>.html` (or `.packed.html` if that name is taken
/// by a file `forge pack` did not write).
pub async fn run(input: &Path, output: Option<&Path>, options: Options) -> Result<Summary> {
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let packer = Packer::new(input, options)?;
    let source = html::decode(&std::fs::read(input).with_context(|| format!("reading {}", input.display()))?);
    let (html, default_name) = match ext.as_str() {
        "html" | "htm" => (html::pack(&packer, &source).await?, format!("{}.packed.html", stem(input))),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "mts" => {
            let shape = component::analyze(&source, input)?;
            let kind = shape.kind();
            let tailwind = packer.options.tailwind.unwrap_or_else(|| shape.wants_tailwind());
            match kind {
                component::Kind::React => packer.note("mounted the default export as a React component"),
                component::Kind::Preact => {
                    packer.note("mounted the default export as a Preact component");
                    *packer.jsx_import_source.lock().unwrap() = Some("preact".into());
                }
                component::Kind::Mount => packer.note("called the exported mount(el) with the page's root element"),
                component::Kind::Script => packer.note("ran the file as the page's module script"),
            }
            if tailwind && packer.options.tailwind.is_none() {
                packer.note(format!(
                    "added Tailwind ({} utility classes in the markup; --no-tailwind to skip)",
                    shape.tailwind_classes
                ));
            }
            let title = packer.options.title.clone().unwrap_or_else(|| component::title_from(input));
            let file = input.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let page = component::page(&file, &title, &kind, tailwind);
            let name = format!("{}.html", stem(input));
            let taken = std::fs::read_to_string(packer.dir.join(&name)).is_ok_and(|t| !t.contains(html::GENERATOR));
            (html::pack(&packer, &page).await?, if taken { format!("{}.packed.html", stem(input)) } else { name })
        }
        _ => bail!("forge pack takes an .html page or a .js/.jsx/.ts/.tsx file (got `{}`)", input.display()),
    };
    let out = output.map(Path::to_path_buf).unwrap_or_else(|| packer.dir.join(default_name));
    if out.canonicalize().ok() == input.canonicalize().ok() {
        bail!("refusing to overwrite the input; pass another -o path");
    }
    std::fs::write(&out, &html).with_context(|| format!("writing {}", out.display()))?;
    let mut summary = packer.summary();
    summary.output = out;
    summary.bytes = html.len();
    summary.input_bytes = source.len();
    Ok(summary)
}

/// `forge pack --watch`: repack on every change next to the input and serve the result with
/// live reload. The output file itself (and other packed files) never trigger a repack.
pub async fn watch(
    input: PathBuf,
    output: Option<PathBuf>,
    options: Options,
    addr: std::net::SocketAddr,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("forge pack --watch → http://{}", listener.local_addr()?);
    watch_on(input, output, options, listener).await
}

pub async fn watch_on(
    input: PathBuf,
    output: Option<PathBuf>,
    options: Options,
    listener: tokio::net::TcpListener,
) -> Result<()> {
    let input = input.canonicalize().with_context(|| format!("{} not found", input.display()))?;
    let dir = input.parent().context("input has no directory")?.to_path_buf();
    let out = Arc::new(Mutex::new(output.clone()));
    let (filter_dir, filter_out) = (dir.clone(), out.clone());
    let filter = move |p: &Path| {
        let Ok(rel) = p.strip_prefix(&filter_dir) else { return false };
        let ignored = rel.components().any(|c| {
            let c = c.as_os_str().to_string_lossy();
            c.starts_with('.')
                || c.ends_with('~')
                || c.ends_with(".swp")
                || ["node_modules", "dist", "target"].contains(&c.as_ref())
        });
        let is_output = filter_out.lock().unwrap().as_deref().is_some_and(|o| o == p)
            || p.to_string_lossy().ends_with(".packed.html");
        !ignored && !is_output
    };
    let roots = vec![(dir.clone(), notify::RecursiveMode::Recursive)];
    crate::server::single::run(listener, roots, filter, move || {
        let (input, options, out) = (input.clone(), options.clone(), out.clone());
        let output = out.lock().unwrap().clone();
        async move {
            let summary = run(&input, output.as_deref(), options).await?;
            for w in &summary.warnings {
                eprintln!("warn: {w}");
            }
            // Remember where it went, so writing it does not trigger another pack.
            *out.lock().unwrap() = Some(summary.output.canonicalize().unwrap_or(summary.output.clone()));
            Ok(std::fs::read_to_string(&summary.output)?)
        }
    })
    .await
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "page".into())
}
