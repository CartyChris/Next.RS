//! Project configuration: `forge.config.json` plus the build target/mode enums.
//!
//! The config file is plain JSON on purpose: reading it never executes user code.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
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
    pub compress: bool,
    /// Extra compile-time replacements, e.g. `{"__VERSION__": "\"1.2.3\""}`.
    pub define: BTreeMap<String, String>,
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
