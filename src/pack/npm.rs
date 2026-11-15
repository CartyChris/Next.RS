//! A small npm installer: registry metadata, npm semver ranges, tarballs verified against
//! their published integrity, and an npm-style hoisted `node_modules` tree.
//!
//! Enough to bundle browser code: no lifecycle scripts (never run), no optional or platform
//! dependencies, no git/URL specs. Tarballs and metadata are cached under the Forge cache.

use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use nodejs_semver::{Range, Version};
use serde::Deserialize;
use sha2::Digest as _;

/// How long cached registry metadata is trusted before it is fetched again.
const META_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_TARBALL: usize = 128 << 20;

#[derive(Debug, Clone, Deserialize)]
struct Packument {
    #[serde(rename = "dist-tags", default)]
    dist_tags: HashMap<String, String>,
    #[serde(default)]
    versions: HashMap<String, Manifest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    version: String,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    peer_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    peer_dependencies_meta: BTreeMap<String, PeerMeta>,
    dist: Dist,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PeerMeta {
    #[serde(default)]
    optional: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct Dist {
    tarball: String,
    integrity: Option<String>,
    shasum: Option<String>,
}

/// A package placed in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
}

pub struct Registry {
    url: String,
    cache: PathBuf,
    client: reqwest::Client,
    offline: bool,
    meta: tokio::sync::Mutex<HashMap<String, Arc<Packument>>>,
}

impl Registry {
    pub fn new(url: &str, cache: PathBuf, client: reqwest::Client, offline: bool) -> Self {
        Self { url: url.trim_end_matches('/').to_string(), cache, client, offline, meta: Default::default() }
    }

    async fn packument(&self, name: &str) -> Result<Arc<Packument>> {
        if let Some(p) = self.meta.lock().await.get(name) {
            return Ok(p.clone());
        }
        let file = self.cache.join("meta").join(format!("{}.json", name.replace('/', "%2f")));
        let fresh = std::fs::metadata(&file)
            .and_then(|m| m.modified())
            .is_ok_and(|t| self.offline || t.elapsed().unwrap_or(META_TTL) < META_TTL);
        let bytes = if fresh {
            std::fs::read(&file)?
        } else if self.offline {
            bail!("{name}: not in the package cache (offline)");
        } else {
            let url = format!("{}/{}", self.url, name.replace('/', "%2f"));
            let res = self
                .client
                .get(&url)
                .header("accept", "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8")
                .send()
                .await
                .with_context(|| format!("fetching {url}"))?;
            if res.status() == reqwest::StatusCode::NOT_FOUND {
                bail!("package `{name}` does not exist on {}", self.url);
            }
            if !res.status().is_success() {
                bail!("{url}: HTTP {}", res.status());
            }
            let bytes = res.bytes().await?.to_vec();
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(&file, &bytes)?;
            bytes
        };
        let p: Packument =
            serde_json::from_slice(&bytes).with_context(|| format!("{name}: invalid registry metadata"))?;
        let p = Arc::new(p);
        self.meta.lock().await.insert(name.to_string(), p.clone());
        Ok(p)
    }

    /// The version `range` selects: the `latest` tag when it satisfies, else the highest match.
    /// Dist-tags (`latest`, `next`, …) work as ranges too.
    async fn select(&self, name: &str, range: &str) -> Result<Manifest> {
        let p = self.packument(name).await?;
        let range = range.trim();
        let tag = if range.is_empty() || range == "*" { Some("latest") } else { p.dist_tags.get(range).map(|_| range) };
        if let Some(v) = tag.and_then(|t| p.dist_tags.get(t)).and_then(|v| p.versions.get(v)) {
            return Ok(v.clone());
        }
        let parsed = Range::parse(range).map_err(|e| anyhow!("{name}@{range}: unsupported version range ({e})"))?;
        if let Some(latest) = p.dist_tags.get("latest").and_then(|v| Version::parse(v).ok()) {
            if parsed.satisfies(&latest) {
                return Ok(p.versions[&latest.to_string()].clone());
            }
        }
        let mut versions: Vec<Version> = p.versions.keys().filter_map(|v| Version::parse(v).ok()).collect();
        versions.sort();
        let best =
            parsed.max_satisfying(&versions).ok_or_else(|| anyhow!("no version of `{name}` matches `{range}`"))?;
        Ok(p.versions[&best.to_string()].clone())
    }

    async fn tarball(&self, name: &str, m: &Manifest) -> Result<Vec<u8>> {
        let file = self.cache.join("tarballs").join(format!("{}-{}.tgz", name.replace('/', "%2f"), m.version));
        if let Ok(bytes) = std::fs::read(&file) {
            if verify(&bytes, &m.dist).is_ok() {
                return Ok(bytes);
            }
        }
        if self.offline {
            bail!("{name}@{}: not in the package cache (offline)", m.version);
        }
        let res =
            self.client.get(&m.dist.tarball).send().await.with_context(|| format!("fetching {}", m.dist.tarball))?;
        if !res.status().is_success() {
            bail!("{}: HTTP {}", m.dist.tarball, res.status());
        }
        let bytes = res.bytes().await?.to_vec();
        if bytes.len() > MAX_TARBALL {
            bail!("{name}@{}: tarball over {} MB", m.version, MAX_TARBALL >> 20);
        }
        verify(&bytes, &m.dist).with_context(|| format!("{name}@{}", m.version))?;
        std::fs::create_dir_all(file.parent().unwrap())?;
        std::fs::write(&file, &bytes)?;
        Ok(bytes)
    }
}

/// Check a tarball against the registry's `integrity` (SRI) or legacy `shasum`.
fn verify(bytes: &[u8], dist: &Dist) -> Result<()> {
    let b64 = |d: &[u8]| base64::engine::general_purpose::STANDARD.encode(d);
    if let Some(sri) = &dist.integrity {
        let mut checked = false;
        for item in sri.split_whitespace() {
            let (alg, want) = item.split_once('-').unwrap_or((item, ""));
            let got = match alg {
                "sha512" => b64(&sha2::Sha512::digest(bytes)),
                "sha384" => b64(&sha2::Sha384::digest(bytes)),
                "sha256" => b64(&sha2::Sha256::digest(bytes)),
                "sha1" => b64(&sha1::Sha1::digest(bytes)),
                _ => continue,
            };
            if got == want {
                return Ok(());
            }
            checked = true;
        }
        if checked {
            bail!("tarball does not match its published integrity");
        }
    }
    if let Some(shasum) = &dist.shasum {
        let got: String = sha1::Sha1::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(shasum) {
            bail!("tarball does not match its published shasum");
        }
        return Ok(());
    }
    bail!("the registry published no integrity for this tarball")
}

/// Unpack a package tarball into `dir`, dropping the archive's top directory (`package/`).
/// Only regular files and directories; nothing may land outside `dir`.
fn extract(tgz: &[u8], dir: &Path) -> Result<()> {
    let tmp = dir.with_extension(format!("tmp{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tgz));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue;
        }
        let path = entry.path()?.into_owned();
        let rel: PathBuf = path.components().skip(1).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            bail!("tarball entry `{}` escapes the package", path.display());
        }
        let out = tmp.join(&rel);
        if kind.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        std::fs::create_dir_all(out.parent().unwrap())?;
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::fs::write(&out, buf)?;
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.parent().unwrap())?;
    std::fs::rename(&tmp, dir)?;
    Ok(())
}

fn installed_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()?.get("version")?.as_str().map(String::from)
}

fn satisfies(version: &str, range: &str) -> bool {
    let range = range.trim();
    if range.is_empty() || range == "*" || range == "latest" {
        return true;
    }
    match (Version::parse(version), Range::parse(range)) {
        (Ok(v), Ok(r)) => r.satisfies(&v),
        // A dist-tag other than latest: whatever is installed is accepted.
        _ => !range.starts_with(|c: char| c.is_ascii_digit() || "^~<>=".contains(c)),
    }
}

/// One dependency to satisfy: `name@range` as seen from package directory `from`, or at a
/// fixed directory (`at`) for packages the page asks for directly.
struct Want {
    name: String,
    range: String,
    from: PathBuf,
    at: Option<PathBuf>,
}

/// Installs into one workspace (`<ws>/node_modules`). Calls may come at any time (the bundler
/// asks for packages as it finds imports); each call adds what is missing and keeps the rest.
pub struct Installer {
    registry: Arc<Registry>,
    ws: PathBuf,
    /// Package directory → version, for everything placed or checked in this run.
    state: tokio::sync::Mutex<HashMap<PathBuf, String>>,
    pub log: std::sync::Mutex<Vec<String>>,
}

impl Installer {
    pub fn new(registry: Arc<Registry>, ws: PathBuf) -> Self {
        Self { registry, ws, state: Default::default(), log: Default::default() }
    }

    pub fn workspace(&self) -> &Path {
        &self.ws
    }

    /// Install `name@range` at the top level (with its dependencies) and return where it is.
    pub async fn root(&self, name: &str, range: &str) -> Result<Placed> {
        validate_name(name)?;
        let mut placed = self.state.lock().await;
        let top = self.ws.join("node_modules").join(name);
        if let Some(v) = placed.get(&top).filter(|v| satisfies(v, range)) {
            return Ok(Placed { name: name.into(), version: v.clone(), dir: top });
        }
        let dir = match self.version_at(&placed, &top) {
            Some(v) if !satisfies(&v, range) => {
                // Two versions asked for directly: keep the first at the top, put this one aside.
                let m = self.registry.select(name, range).await?;
                self.log(format!("{name}@{} kept beside {name}@{v}", m.version));
                self.ws
                    .join("node_modules/.forge")
                    .join(format!("{}@{}", name.replace('/', "+"), m.version))
                    .join("node_modules")
                    .join(name)
            }
            _ => top,
        };
        let want = Want { name: name.into(), range: range.into(), from: self.ws.clone(), at: Some(dir.clone()) };
        self.install(&mut placed, vec![want]).await?;
        Ok(Placed { name: name.into(), version: placed[&dir].clone(), dir })
    }

    fn log(&self, line: String) {
        self.log.lock().unwrap().push(line);
    }

    /// Breadth-first in waves, npm style: a dependency already visible from the package (its
    /// own `node_modules`, then each ancestor's) is reused when its version fits; otherwise it
    /// goes to the top level when that is free, or nested under the package that needs it.
    /// Each wave fetches metadata and tarballs concurrently.
    async fn install(&self, placed: &mut HashMap<PathBuf, String>, mut wave: Vec<Want>) -> Result<()> {
        while !wave.is_empty() {
            let manifests =
                futures_util::future::join_all(wave.iter().map(|w| self.registry.select(&w.name, &w.range))).await;
            let mut next = Vec::new();
            let mut fetch = Vec::new();
            for (w, m) in wave.into_iter().zip(manifests) {
                let m = m?;
                let found = match &w.at {
                    Some(at) => self.version_at(placed, at).map(|v| (at.clone(), v)),
                    None => self.visible(placed, &w.from, &w.name),
                };
                let (dir, manifest) = match found {
                    Some((dir, v)) if satisfies(&v, &w.range) => {
                        if placed.contains_key(&dir) {
                            continue;
                        }
                        // On disk from an earlier run: check its dependencies too.
                        let exact = if v == m.version { m } else { self.registry.select(&w.name, &v).await? };
                        placed.insert(dir.clone(), v);
                        (dir, exact)
                    }
                    _ => {
                        let top = self.ws.join("node_modules").join(&w.name);
                        let dir = match w.at {
                            Some(at) => at,
                            None if self.version_at(placed, &top).is_none() => top,
                            None => w.from.join("node_modules").join(&w.name),
                        };
                        placed.insert(dir.clone(), m.version.clone());
                        if installed_version(&dir).as_deref() != Some(m.version.as_str()) {
                            fetch.push((w.name.clone(), m.clone(), dir.clone()));
                        }
                        (dir, m)
                    }
                };
                let optional = |n: &String| manifest.peer_dependencies_meta.get(n).is_some_and(|p| p.optional);
                let peers = manifest.peer_dependencies.iter().filter(|(n, _)| !optional(n));
                for (name, range) in manifest.dependencies.iter().chain(peers) {
                    if validate_name(name).is_err() || !is_registry_range(range) {
                        self.log(format!("skipped {name}@{range} (not a registry dependency)"));
                        continue;
                    }
                    next.push(Want { name: name.clone(), range: range.clone(), from: dir.clone(), at: None });
                }
            }
            let jobs = fetch.into_iter().map(|(name, m, dir)| async move {
                let tgz = self.registry.tarball(&name, &m).await?;
                let label = format!("{name}@{}", m.version);
                tokio::task::spawn_blocking(move || extract(&tgz, &dir).with_context(|| format!("unpacking {label}")))
                    .await??;
                self.log(format!("{name}@{}", m.version));
                anyhow::Ok(())
            });
            for r in futures_util::future::join_all(jobs).await {
                r?;
            }
            wave = next;
        }
        Ok(())
    }

    fn version_at(&self, placed: &HashMap<PathBuf, String>, dir: &Path) -> Option<String> {
        placed.get(dir).cloned().or_else(|| installed_version(dir))
    }

    /// Where Node resolution from package directory `from` finds `name`, and its version.
    fn visible(&self, placed: &HashMap<PathBuf, String>, from: &Path, name: &str) -> Option<(PathBuf, String)> {
        let mut dir = Some(from);
        while let Some(d) = dir {
            if d.file_name().is_none_or(|n| n != "node_modules") {
                let candidate = d.join("node_modules").join(name);
                if let Some(v) = self.version_at(placed, &candidate) {
                    return Some((candidate, v));
                }
            }
            if d == self.ws {
                break;
            }
            dir = d.parent().filter(|p| p.starts_with(&self.ws));
        }
        None
    }
}

/// npm package names: optional `@scope/`, then URL-safe characters, no traversal.
pub fn validate_name(name: &str) -> Result<()> {
    let ok_part = |p: &str| {
        !p.is_empty() && !p.starts_with('.') && p.chars().all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c))
    };
    let valid = match name.strip_prefix('@') {
        Some(rest) => rest.split_once('/').is_some_and(|(s, n)| ok_part(s) && ok_part(n)),
        None => ok_part(name),
    };
    if valid && name.len() <= 214 { Ok(()) } else { bail!("invalid package name `{name}`") }
}

fn is_registry_range(range: &str) -> bool {
    !(range.contains(':') || range.contains('/'))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A gzipped tarball with `package/<path>` entries.
    pub fn tarball(files: &[(&str, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (path, body) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, format!("package/{path}"), body.as_bytes()).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    pub fn sri(bytes: &[u8]) -> String {
        format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(bytes)))
    }

    #[test]
    fn integrity_is_enforced() {
        let tgz = tarball(&[("package.json", r#"{"name":"a","version":"1.0.0"}"#)]);
        let good = Dist { tarball: String::new(), integrity: Some(sri(&tgz)), shasum: None };
        assert!(verify(&tgz, &good).is_ok());
        let mut tampered = tgz.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(verify(&tampered, &good).is_err());
        let none = Dist { tarball: String::new(), integrity: None, shasum: None };
        assert!(verify(&tgz, &none).is_err());
        let sha1: String = sha1::Sha1::digest(&tgz).iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify(&tgz, &Dist { tarball: String::new(), integrity: None, shasum: Some(sha1) }).is_ok());
    }

    #[test]
    fn extraction_strips_the_top_directory_and_refuses_traversal() {
        let dir = std::env::temp_dir().join(format!("forge-npm-extract-{}", std::process::id()));
        let tgz = tarball(&[("package.json", "{}"), ("lib/index.js", "export {}")]);
        extract(&tgz, &dir.join("pkg")).unwrap();
        assert!(dir.join("pkg/lib/index.js").is_file() && dir.join("pkg/package.json").is_file());
        // tar::Builder refuses `..` paths, so write the header bytes by hand.
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..17].copy_from_slice(b"package/../../evi");
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        let mut raw = header.as_bytes().to_vec();
        raw.extend_from_slice(&[b'x'; 512]);
        raw.extend_from_slice(&[0; 1024]);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &raw).unwrap();
        let err = extract(&gz.finish().unwrap(), &dir.join("evil")).unwrap_err();
        assert!(err.to_string().contains("escapes"), "{err}");
        assert!(!dir.join("evi").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn names_and_ranges() {
        for ok in ["react", "@react-three/fiber", "lodash.debounce", "three"] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "../x", "@scope", "@/x", "a/b", ".bin", "a b"] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
        assert!(satisfies("18.3.1", "^18") && !satisfies("19.0.0", "^18") && satisfies("1.2.3", "*"));
        assert!(satisfies("0.160.1", "~0.160.0") && satisfies("2.0.0", ">=1 <3 || 5"));
        assert!(!is_registry_range("github:a/b") && is_registry_range("^1.0.0"));
    }
}

#[cfg(test)]
mod install_tests {
    use super::tests::{sri, tarball};
    use super::*;
    use axum::{Router, extract::Path as UrlPath, response::IntoResponse};

    type Pkg = (&'static str, &'static str, &'static str); // name, version, package.json extras

    /// A registry on localhost serving `pkgs`; returns its URL. Requests are counted in `hits`.
    pub async fn fake_registry(
        pkgs: &'static [Pkg],
        latest: &'static [(&'static str, &'static str)],
        hits: Arc<std::sync::atomic::AtomicUsize>,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let tgz = |name: &str, version: &str, extra: &str| {
            let json = format!(r#"{{"name":"{name}","version":"{version}"{extra}}}"#);
            tarball(&[("package.json", json.as_str()), ("index.js", &format!("export default '{name}@{version}';"))])
        };
        let b = base.clone();
        let app = Router::new()
            .route(
                "/{name}",
                axum::routing::get(move |UrlPath(name): UrlPath<String>| {
                    let b = b.clone();
                    async move {
                        let mut versions = serde_json::Map::new();
                        for (n, v, extra) in pkgs.iter().filter(|(n, _, _)| *n == name) {
                            let manifest: serde_json::Value = serde_json::from_str(&format!(r#"{{"name":"{n}","version":"{v}"{extra}}}"#)).unwrap();
                            let mut m = manifest.as_object().unwrap().clone();
                            let bytes = tgz(n, v, extra);
                            m.insert("dist".into(), serde_json::json!({ "tarball": format!("{b}/-/{n}-{v}.tgz"), "integrity": sri(&bytes) }));
                            versions.insert(v.to_string(), m.into());
                        }
                        if versions.is_empty() {
                            return (axum::http::StatusCode::NOT_FOUND, String::new()).into_response();
                        }
                        let tag = latest.iter().find(|(n, _)| *n == name).map(|(_, v)| *v).unwrap();
                        serde_json::json!({ "name": name, "dist-tags": { "latest": tag }, "versions": versions }).to_string().into_response()
                    }
                }),
            )
            .route(
                "/-/{file}",
                axum::routing::get(move |UrlPath(file): UrlPath<String>| {
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let stem = file.trim_end_matches(".tgz");
                        let (n, v) = stem.rsplit_once('-').unwrap();
                        let (_, _, extra) = pkgs.iter().find(|(pn, pv, _)| *pn == n && *pv == v).unwrap();
                        let mut bytes = tgz(n, v, extra);
                        if n == "tampered" {
                            *bytes.last_mut().unwrap() ^= 1;
                        }
                        bytes.into_response()
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    const PKGS: &[Pkg] = &[
        ("app-lib", "1.0.0", r#","dependencies":{"dep":"^1","shared":"^2"}"#),
        ("dep", "1.2.0", ""),
        ("dep", "1.5.0", ""),
        ("dep", "2.0.0", ""),
        ("shared", "1.0.0", ""),
        ("shared", "2.1.0", ""),
        ("other", "1.0.0", r#","dependencies":{"shared":"^1"}"#),
        (
            "plugin",
            "1.0.0",
            r#","peerDependencies":{"host":"^3","opt":"*"},"peerDependenciesMeta":{"opt":{"optional":true}}"#,
        ),
        ("host", "3.0.0", ""),
        ("tampered", "1.0.0", ""),
    ];
    const LATEST: &[(&str, &str)] = &[
        ("app-lib", "1.0.0"),
        ("dep", "1.2.0"),
        ("shared", "2.1.0"),
        ("other", "1.0.0"),
        ("plugin", "1.0.0"),
        ("host", "3.0.0"),
        ("tampered", "1.0.0"),
    ];

    #[tokio::test]
    async fn installs_a_hoisted_tree() {
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let url = fake_registry(PKGS, LATEST, hits.clone()).await;
        let root = std::env::temp_dir().join(format!("forge-npm-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let registry = Arc::new(Registry::new(&url, root.join("cache"), client.clone(), false));
        let ws = root.join("ws");
        let installer = Installer::new(registry.clone(), ws.clone());

        let lib = installer.root("app-lib", "^1").await.unwrap();
        assert_eq!((lib.version.as_str(), lib.dir.clone()), ("1.0.0", ws.join("node_modules/app-lib")));
        // `latest` (1.2.0) satisfies ^1, so it wins over the higher 1.5.0, as with npm.
        assert_eq!(installed_version(&ws.join("node_modules/dep")).as_deref(), Some("1.2.0"));
        assert_eq!(installed_version(&ws.join("node_modules/shared")).as_deref(), Some("2.1.0"));
        // A conflicting range nests under the package that needs it.
        installer.root("other", "1").await.unwrap();
        assert_eq!(installed_version(&ws.join("node_modules/other/node_modules/shared")).as_deref(), Some("1.0.0"));
        // Required peers are installed, optional ones are not.
        installer.root("plugin", "^1").await.unwrap();
        assert!(ws.join("node_modules/host/index.js").is_file() && !ws.join("node_modules/opt").exists());
        // A second direct version is kept aside instead of replacing the first.
        let dep2 = installer.root("dep", "2").await.unwrap();
        assert_eq!(dep2.version, "2.0.0");
        assert!(dep2.dir.ends_with("node_modules/.forge/dep@2.0.0/node_modules/dep"), "{}", dep2.dir.display());
        assert_eq!(installed_version(&ws.join("node_modules/dep")).as_deref(), Some("1.2.0"));
        let downloads = hits.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(downloads, 8);

        // A fresh run over the same workspace downloads nothing.
        let again =
            Installer::new(Arc::new(Registry::new(&url, root.join("cache"), client.clone(), false)), ws.clone());
        for (n, r) in [("app-lib", "^1"), ("other", "1"), ("plugin", "^1")] {
            again.root(n, r).await.unwrap();
        }
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), downloads);
        assert!(again.log.lock().unwrap().is_empty(), "{:?}", again.log.lock().unwrap());

        // Tampered tarballs are rejected and nothing is left behind.
        let err = installer.root("tampered", "1").await.unwrap_err();
        assert!(format!("{err:#}").contains("integrity"), "{err:#}");
        assert!(!ws.join("node_modules/tampered").exists());
        // Unknown packages say so.
        let err = installer.root("nope", "1").await.unwrap_err();
        assert!(format!("{err:#}").contains("does not exist"), "{err:#}");
        std::fs::remove_dir_all(root).ok();
    }
}
