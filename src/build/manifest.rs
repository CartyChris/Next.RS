//! Build artifacts that describe a build: `manifest.json` (what the runtime loads),
//! `capsule.json` (integrity of every output file), `bundle-stats.json` (sizes).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{bundle::Bundle, generated::PageKind};
use crate::{
    config::{Mode, Target},
    routes::{RouteKind, Segment},
};

pub const SCHEMA: u32 = 1;

/// What a build target can do at runtime. Single-file and static builds never claim SSR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub ssr: bool,
    pub streaming_ssr: bool,
    pub route_data_endpoint: bool,
    pub server_loaders: bool,
    pub api_routes: bool,
    pub prerendered: bool,
    pub client_hydration: bool,
    pub client_navigation: bool,
    pub persistent_server_cache: bool,
    pub single_file: bool,
    pub wasm_host: bool,
}

impl Capabilities {
    pub fn for_target(target: Target) -> Self {
        let server = target == Target::Server;
        Self {
            ssr: server,
            streaming_ssr: server,
            route_data_endpoint: server,
            server_loaders: server,
            api_routes: server,
            prerendered: matches!(target, Target::Static | Target::SingleHtml),
            client_hydration: true,
            client_navigation: true,
            persistent_server_cache: server,
            single_file: target == Target::SingleHtml,
            wasm_host: target == Target::WasmHost,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteEntry {
    pub id: String,
    pub pattern: String,
    pub segments: Vec<Segment>,
    pub kind: RouteKind,
    pub file: String,
    /// Pages only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<PageKind>,
    /// Rendered on the server (false: the client renders it into an empty shell).
    #[serde(default)]
    pub ssr: bool,
    /// Seconds a rendered page may be served from the page cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revalidate: Option<f64>,
    /// Stream HTML as Suspense boundaries resolve (default) or send it complete.
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub loader: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientAssets {
    /// URL of the client entry script.
    pub entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
    /// Route id → module URLs to `modulepreload` on first load.
    pub preload: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32,
    pub forge_version: String,
    pub build_id: String,
    pub target: Target,
    pub mode: Mode,
    pub capabilities: Capabilities,
    pub lang: String,
    pub title: String,
    /// Most specific first; first match wins.
    pub routes: Vec<RouteEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_found: Option<RouteEntry>,
    pub client: ClientAssets,
    /// Relative to the output directory.
    pub server_entry: String,
}

impl Manifest {
    pub fn load(out_dir: &Path) -> Result<Self> {
        let path = out_dir.join("manifest.json");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("{} not found; run `forge build` first", path.display()))?;
        let m: Manifest = serde_json::from_str(&text).with_context(|| format!("invalid {}", path.display()))?;
        if m.schema != SCHEMA {
            bail!("{} has schema {} but this forge reads schema {SCHEMA}; rebuild", path.display(), m.schema);
        }
        Ok(m)
    }

    pub fn page(&self, id: &str) -> Option<&RouteEntry> {
        self.routes.iter().find(|r| r.id == id).or(self.not_found.as_ref().filter(|r| r.id == id))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapsuleFile {
    pub path: String,
    pub bytes: u64,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capsule {
    pub build_id: String,
    pub files: Vec<CapsuleFile>,
}

impl Capsule {
    /// Hash every file under `dir` listed in `rel_paths`.
    pub fn of(build_id: &str, dir: &Path, rel_paths: &[String]) -> Result<Self> {
        let mut files = Vec::with_capacity(rel_paths.len());
        for rel in rel_paths {
            let bytes = std::fs::read(dir.join(rel)).with_context(|| format!("reading {rel}"))?;
            files.push(CapsuleFile {
                path: rel.clone(),
                bytes: bytes.len() as u64,
                blake3: blake3::hash(&bytes).to_hex().to_string(),
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self { build_id: build_id.to_string(), files })
    }

    /// Every listed file exists with the recorded hash.
    pub fn verify(&self, dir: &Path) -> Result<()> {
        for f in &self.files {
            let bytes = std::fs::read(dir.join(&f.path)).with_context(|| format!("capsule: {} is missing", f.path))?;
            if blake3::hash(&bytes).to_hex().as_str() != f.blake3 {
                bail!("capsule: {} was modified after the build", f.path);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkStats {
    pub file: String,
    pub bytes: usize,
    pub gzip: usize,
    /// Largest contributing modules: `(module, rendered bytes)`.
    pub modules: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleStats {
    pub client: Vec<ChunkStats>,
    pub server: Vec<ChunkStats>,
    pub client_bytes: usize,
    pub client_gzip: usize,
    pub css_bytes: usize,
    pub asset_bytes: usize,
}

impl BundleStats {
    pub fn of(client: Option<&Bundle>, server: &Bundle, root: &Path) -> Self {
        let chunks = |b: &Bundle| -> Vec<ChunkStats> {
            b.chunks
                .iter()
                .map(|c| {
                    let mut modules: Vec<(String, usize)> = c
                        .modules
                        .iter()
                        .filter(|(_, n)| *n > 0)
                        .map(|(m, n)| {
                            (
                                Path::new(m)
                                    .strip_prefix(root)
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .unwrap_or_else(|_| m.clone()),
                                *n,
                            )
                        })
                        .collect();
                    modules.sort_by_key(|m| std::cmp::Reverse(m.1));
                    modules.truncate(12);
                    ChunkStats { file: c.file.clone(), bytes: c.code.len(), gzip: gzip_len(c.code.as_bytes()), modules }
                })
                .collect()
        };
        let mut stats = BundleStats { server: chunks(server), ..Default::default() };
        if let Some(client) = client {
            stats.client = chunks(client);
            stats.client_bytes = stats.client.iter().map(|c| c.bytes).sum();
            stats.client_gzip = stats.client.iter().map(|c| c.gzip).sum();
            stats.css_bytes = client.css.len();
            stats.asset_bytes = client.files.iter().map(|(_, b)| b.len()).sum();
        }
        stats
    }
}

pub fn gzip_len(bytes: &[u8]) -> usize {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = enc.write_all(bytes);
    enc.finish().map(|v| v.len()).unwrap_or(0)
}

/// Write `.br` and `.gz` siblings for compressible files, in parallel.
pub fn precompress(dir: &Path, rel_paths: &[String]) -> Result<Vec<String>> {
    const EXTS: &[&str] = &["js", "css", "svg", "json", "wasm", "html", "txt", "map", "glsl", "gltf"];
    let targets: Vec<PathBuf> = rel_paths
        .iter()
        .map(|r| dir.join(r))
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTS.contains(&e)))
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() >= 1024))
        .collect();
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get());
    let results: Vec<Result<()>> = std::thread::scope(|s| {
        let handles: Vec<_> = targets
            .chunks(targets.len().div_ceil(threads).max(1))
            .map(|batch| {
                s.spawn(move || -> Result<()> {
                    for path in batch {
                        let bytes = std::fs::read(path)?;
                        let mut br = Vec::new();
                        {
                            use std::io::Write;
                            let quality = if bytes.len() > 4 << 20 { 6 } else { 11 };
                            let mut w = brotli::CompressorWriter::new(&mut br, 64 * 1024, quality, 22);
                            w.write_all(&bytes)?;
                        }
                        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
                        std::io::Write::write_all(&mut gz, &bytes)?;
                        let gz = gz.finish()?;
                        let name = path.file_name().unwrap().to_string_lossy();
                        if br.len() < bytes.len() {
                            std::fs::write(path.with_file_name(format!("{name}.br")), br)?;
                        }
                        if gz.len() < bytes.len() {
                            std::fs::write(path.with_file_name(format!("{name}.gz")), gz)?;
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("compression thread")).collect()
    });
    results.into_iter().collect::<Result<()>>()?;
    let mut written = Vec::new();
    for rel in rel_paths {
        for ext in ["br", "gz"] {
            let candidate = format!("{rel}.{ext}");
            if dir.join(&candidate).is_file() {
                written.push(candidate);
            }
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_per_target() {
        let server = Capabilities::for_target(Target::Server);
        assert!(
            server.ssr
                && server.streaming_ssr
                && server.route_data_endpoint
                && server.server_loaders
                && !server.single_file
        );
        let single = Capabilities::for_target(Target::SingleHtml);
        assert!(
            !single.ssr
                && !single.server_loaders
                && !single.route_data_endpoint
                && single.single_file
                && single.prerendered
        );
        assert!(single.client_hydration && single.client_navigation);
        let stat = Capabilities::for_target(Target::Static);
        assert!(!stat.ssr && !stat.single_file && stat.prerendered);
        assert!(Capabilities::for_target(Target::WasmHost).wasm_host);
    }

    #[test]
    fn capsule_detects_tampering() {
        let dir = std::env::temp_dir().join(format!("forge-capsule-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.js"), "1").unwrap();
        let c = Capsule::of("b1", &dir, &["a.js".into()]).unwrap();
        c.verify(&dir).unwrap();
        std::fs::write(dir.join("a.js"), "2").unwrap();
        assert!(c.verify(&dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn precompress_writes_smaller_siblings() {
        let dir = std::env::temp_dir().join(format!("forge-precompress-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("big.js"), "console.log('hello');\n".repeat(200)).unwrap();
        std::fs::write(dir.join("tiny.js"), "1").unwrap();
        let written = precompress(&dir, &["big.js".into(), "tiny.js".into()]).unwrap();
        assert_eq!(written, ["big.js.br", "big.js.gz"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
