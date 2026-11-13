//! Project configuration: `forge.config.json` plus the build target/mode enums.
//!
//! The config file is plain JSON on purpose: reading it never executes user code.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "forge.config.json";

/// What a build produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    /// SSR app served by `forge start`.
    Server,
    /// Prerendered multi-file site for any static host.
    Static,
    /// One self-contained `index.html`.
    SingleHtml,
    /// Planned. Selecting it is an error until it exists.
    WasmHost,
}

impl Target {
    pub fn as_str(self) -> &'static str {
        match self {
            Target::Server => "server",
            Target::Static => "static",
            Target::SingleHtml => "single-html",
            Target::WasmHost => "wasm-host",
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Target {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "server" => Target::Server,
            "static" => Target::Static,
            "single-html" | "single" | "html" => Target::SingleHtml,
            "wasm-host" => Target::WasmHost,
            other => bail!("unknown target `{other}` (expected server, static, single-html)"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Dev,
    Prod,
}

impl Mode {
    pub fn is_dev(self) -> bool {
        self == Mode::Dev
    }
    pub fn node_env(self) -> &'static str {
        if self.is_dev() { "development" } else { "production" }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub app_dir: String,
    pub public_dir: String,
    /// Build cache + server artifacts.
    pub out_dir: String,
    /// Output of `static` and `single-html` targets.
    pub dist_dir: String,
    /// Default document title when a page exports no `meta.title`.
    pub title: String,
    pub lang: String,
    pub server: ServerConfig,
    pub security: SecurityConfig,
    pub build: BuildConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app_dir: "app".into(),
            public_dir: "public".into(),
            out_dir: ".forge".into(),
            dist_dir: "dist".into(),
            title: "Forge App".into(),
            lang: "en".into(),
            server: ServerConfig::default(),
            security: SecurityConfig::default(),
            build: BuildConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// JS worker threads. 0 = one per core (max 8).
    pub workers: usize,
    /// Environment variables server code may read via `ctx.env`. Nothing else is visible.
    pub env: Vec<String>,
    /// Hosts server-side `fetch()` may reach (`api.example.com`, `*.example.com`, or `*`).
    /// Empty = no outbound network.
    pub allow_fetch: Vec<String>,
    /// Per-worker JS heap cap.
    pub memory_limit_mb: usize,
    /// Wall-clock budget for a request before the client gets a 504.
    pub timeout_ms: u64,
    /// Longest uninterrupted synchronous JS run before the worker is recycled.
    pub cpu_ms: u64,
    pub max_body_bytes: usize,
    pub page_cache_entries: usize,
    /// One line per request on stdout (always on in `forge dev`).
    pub access_log: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 3000,
            workers: 0,
            env: Vec::new(),
            allow_fetch: Vec::new(),
            memory_limit_mb: 256,
            timeout_ms: 15_000,
            cpu_ms: 2_000,
            max_body_bytes: 1 << 20,
            page_cache_entries: 1024,
            access_log: false,
        }
    }
}

impl ServerConfig {
    pub fn worker_count(&self) -> usize {
        match self.workers {
            0 => std::thread::available_parallelism().map_or(1, |n| n.get()).min(8),
            n => n,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct SecurityConfig {
    /// Allow `eval` / `new Function` in app source. Off by default.
    pub allow_eval: bool,
    /// `false` disables the Content-Security-Policy header, a string replaces it.
    pub csp: Option<Csp>,
    /// Extra origins appended to the default CSP's `connect-src`.
    pub connect_src: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Csp {
    Enabled(bool),
    Custom(String),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildConfig {
    /// Defaults to true in production builds.
    pub minify: Option<bool>,
    /// Defaults to true in dev builds.
    pub sourcemap: Option<bool>,
    /// single-html: ship the script deflated and inflate it in the browser.
    /// `"auto"` (default) does so when it makes the file meaningfully smaller.
    pub compress: Compress,
    /// Extra compile-time replacements, e.g. `{"__VERSION__": "\"1.2.3\""}`.
    pub define: BTreeMap<String, String>,
}

impl Config {
    /// Directories are relative paths inside the project. Build outputs are deleted and
    /// rewritten on every build, so they may not be the project itself or overlap sources.
    pub fn check_dirs(&self) -> Result<()> {
        let rel = |name: &str, v: &str| -> Result<PathBuf> {
            let p = Path::new(v);
            let inside =
                !p.is_absolute() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
            let norm: PathBuf = p.components().filter(|c| matches!(c, Component::Normal(_))).collect();
            if !inside || norm.as_os_str().is_empty() {
                bail!(
                    "`{name}` must be a folder inside the project, like \"{}\" (got {v:?})",
                    name.trim_end_matches("Dir")
                );
            }
            Ok(norm)
        };
        let sources = [("appDir", rel("appDir", &self.app_dir)?), ("publicDir", rel("publicDir", &self.public_dir)?)];
        let outputs = [("outDir", rel("outDir", &self.out_dir)?), ("distDir", rel("distDir", &self.dist_dir)?)];
        let overlap = |a: &Path, b: &Path| a.starts_with(b) || b.starts_with(a);
        for (out_name, out) in &outputs {
            for (src_name, src) in &sources {
                if overlap(out, src) {
                    bail!(
                        "`{out_name}` ({}) overlaps `{src_name}` ({}): build output is deleted and rewritten",
                        out.display(),
                        src.display()
                    );
                }
            }
        }
        if overlap(&outputs[0].1, &outputs[1].1) {
            bail!("`outDir` and `distDir` must not overlap");
        }
        Ok(())
    }
}

/// `build.compress`: `true`, `false` or `"auto"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Compress {
    #[default]
    Auto,
    On,
    Off,
}

impl Serialize for Compress {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Compress::Auto => s.serialize_str("auto"),
            Compress::On => s.serialize_bool(true),
            Compress::Off => s.serialize_bool(false),
        }
    }
}

impl<'de> Deserialize<'de> for Compress {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bool(bool),
            Str(String),
        }
        match Raw::deserialize(d)? {
            Raw::Bool(true) => Ok(Compress::On),
            Raw::Bool(false) => Ok(Compress::Off),
            Raw::Str(s) if s == "auto" => Ok(Compress::Auto),
            Raw::Str(s) => Err(serde::de::Error::custom(format!("expected true, false or \"auto\", found {s:?}"))),
        }
    }
}

/// A resolved project: root directory + config + the knobs a single build needs.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: Config,
    pub target: Target,
    pub mode: Mode,
}

impl Project {
    pub fn load(root: impl AsRef<Path>, target: Target, mode: Mode) -> Result<Self> {
        let root = root.as_ref();
        let root = root.canonicalize().with_context(|| format!("project root `{}` not found", root.display()))?;
        let path = root.join(CONFIG_FILE);
        let config = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("invalid {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        config.check_dirs().with_context(|| format!("invalid {}", path.display()))?;
        Ok(Self { root, config, target, mode })
    }

    pub fn app_dir(&self) -> PathBuf {
        self.root.join(&self.config.app_dir)
    }
    pub fn public_dir(&self) -> PathBuf {
        self.root.join(&self.config.public_dir)
    }
    pub fn out_dir(&self) -> PathBuf {
        self.root.join(&self.config.out_dir)
    }
    pub fn dist_dir(&self) -> PathBuf {
        self.root.join(&self.config.dist_dir)
    }
    pub fn generated_dir(&self) -> PathBuf {
        self.out_dir().join("generated")
    }
    /// The variables listed in `server.env`, from the process environment or else `.env`.
    pub fn server_env(&self) -> std::collections::HashMap<String, String> {
        let file = std::fs::read_to_string(self.root.join(".env")).map(|t| parse_dotenv(&t)).unwrap_or_default();
        self.config
            .server
            .env
            .iter()
            .filter_map(|k| std::env::var(k).ok().or_else(|| file.get(k).cloned()).map(|v| (k.clone(), v)))
            .collect()
    }

    pub fn minify(&self) -> bool {
        self.config.build.minify.unwrap_or(!self.mode.is_dev())
    }
    pub fn sourcemap(&self) -> bool {
        self.config.build.sourcemap.unwrap_or(self.mode.is_dev()) && self.target == Target::Server
    }
}

/// `KEY=value` lines; `#` comments, optional `export `, optional matching quotes.
pub fn parse_dotenv(text: &str) -> std::collections::HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.strip_prefix("export ").unwrap_or(line).split_once('=')?;
            let v = v.trim();
            let v = match (v.chars().next(), v.chars().last()) {
                (Some(q @ ('"' | '\'')), Some(e)) if q == e && v.len() >= 2 => &v[1..v.len() - 1],
                _ => v.split(" #").next().unwrap_or(v).trim(),
            };
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_partial_override() {
        let c: Config = serde_json::from_str(r#"{"server":{"port":8080,"allowFetch":["*"]}}"#).unwrap();
        assert_eq!(c.server.port, 8080);
        assert_eq!(c.server.host, "127.0.0.1");
        assert_eq!(c.app_dir, "app");
        assert!(!c.security.allow_eval);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(serde_json::from_str::<Config>(r#"{"sever":{}}"#).is_err());
    }

    #[test]
    fn csp_accepts_bool_or_string() {
        let c: Config = serde_json::from_str(r#"{"security":{"csp":false}}"#).unwrap();
        assert!(matches!(c.security.csp, Some(Csp::Enabled(false))));
        let c: Config = serde_json::from_str(r#"{"security":{"csp":"default-src 'none'"}}"#).unwrap();
        assert!(matches!(c.security.csp, Some(Csp::Custom(_))));
    }

    #[test]
    fn directories_stay_inside_the_project() {
        let with = |field: &str, value: &str| {
            let c: Config =
                serde_json::from_str(&format!(r#"{{"{field}":{}}}"#, serde_json::to_string(value).unwrap())).unwrap();
            c.check_dirs()
        };
        assert!(Config::default().check_dirs().is_ok());
        assert!(with("distDir", "build/site").is_ok() && with("distDir", "./out").is_ok());
        for bad in ["/", "/tmp/x", "..", "../elsewhere", "dist/../..", ".", "", "app", "app/dist", ".forge/dist"] {
            assert!(with("distDir", bad).is_err(), "distDir {bad:?}");
        }
        assert!(
            with("outDir", "/var/lib").is_err()
                && with("outDir", "public").is_err()
                && with("appDir", "../src").is_err()
        );
        assert!(with("publicDir", ".").is_err(), "the whole project must not be public");
    }

    #[test]
    fn compress_accepts_bool_or_auto() {
        let parse = |j: &str| serde_json::from_str::<Config>(&format!(r#"{{"build":{{"compress":{j}}}}}"#));
        assert_eq!(Config::default().build.compress, Compress::Auto);
        assert_eq!(parse("true").unwrap().build.compress, Compress::On);
        assert_eq!(parse("false").unwrap().build.compress, Compress::Off);
        assert_eq!(parse(r#""auto""#).unwrap().build.compress, Compress::Auto);
        assert!(parse(r#""yes""#).unwrap_err().to_string().contains("expected true, false or \"auto\""));
    }

    #[test]
    fn dotenv() {
        let env = parse_dotenv("# c\nA=1\nexport B=\"two words\"\nC='x' \nD=y # note\nbad line\n");
        assert_eq!(env["A"], "1");
        assert_eq!(env["B"], "two words");
        assert_eq!(env["C"], "x");
        assert_eq!(env["D"], "y");
        assert_eq!(env.len(), 4);
    }

    #[test]
    fn target_parsing() {
        assert_eq!("single-html".parse::<Target>().unwrap(), Target::SingleHtml);
        assert!("nope".parse::<Target>().is_err());
    }
}
