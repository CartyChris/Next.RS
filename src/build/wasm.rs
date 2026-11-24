//! Opt-in Cargo compilation for browser-side `forge:wasm/<name>` imports.
//!
//! Cargo and build scripts run with ordinary host permissions. This is a build
//! input, not execution in the QuickJS application sandbox.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::process::Command;

use crate::config::{Project, WasmConfig};

const TARGET: &str = "wasm32-unknown-unknown";
const BUILD_TIMEOUT: Duration = Duration::from_secs(600);
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(super) fn aliases(project: &Project) -> impl Iterator<Item = (String, PathBuf)> + '_ {
    project.config.build.wasm.iter().map(|module| {
        (format!("forge:wasm/{}", module.name), project.out_dir().join("wasm").join(format!("{}.wasm", module.name)))
    })
}

/// Run before the bundler reads an alias. Any compile error aborts the build;
/// an earlier successful artifact is never silently substituted for failed code.
pub async fn compile(project: &Project) -> Result<()> {
    // Public callers may construct Project without going through Project::load.
    project.config.check_dirs()?;
    for module in &project.config.build.wasm {
        compile_module(project, module).await.with_context(|| format!("Rust/WASM module `{}`", module.name))?;
    }
    Ok(())
}

fn manifest_path(project: &Project, module: &WasmConfig) -> Result<PathBuf> {
    let manifest = project
        .root
        .join(&module.manifest_path)
        .canonicalize()
        .with_context(|| format!("Cargo manifest `{}` not found", module.manifest_path))?;
    if !manifest.starts_with(&project.root) {
        bail!("manifestPath `{}` resolves outside the project", module.manifest_path);
    }
    if !manifest.is_file() {
        bail!("manifestPath `{}` is not a file", module.manifest_path);
    }
    for output in [project.out_dir(), project.dist_dir()] {
        let canonical = output.canonicalize().unwrap_or(output);
        if manifest.starts_with(canonical) {
            bail!("manifestPath must not resolve inside a build output directory");
        }
    }
    Ok(manifest)
}

/// Reject symlinks in directories owned by the compiler before creating files.
fn cache_dir(project: &Project, path: &Path) -> Result<PathBuf> {
    let relative = path.strip_prefix(&project.root).context("WASM cache must be inside the project")?;
    let mut current = project.root.clone();
    for component in relative.components() {
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                bail!("WASM output path must not contain a symlink: {}", current.display())
            }
            Ok(meta) if !meta.is_dir() => bail!("WASM output path is not a directory: {}", current.display()),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).with_context(|| format!("creating {}", current.display()))?;
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", current.display())),
        }
    }
    Ok(current)
}

#[derive(Deserialize)]
struct CargoMessage {
    reason: String,
    #[serde(default)]
    manifest_path: Option<PathBuf>,
    #[serde(default)]
    target: Option<CargoTarget>,
    #[serde(default)]
    filenames: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct CargoTarget {
    #[serde(default)]
    crate_types: Vec<String>,
}

fn artifact(stdout: &[u8], manifest: &Path) -> Result<PathBuf> {
    let mut artifacts = Vec::new();
    for line in stdout.split(|b| *b == b'\n') {
        // Build scripts may print non-JSON output; Cargo documents this behavior.
        let Ok(message) = serde_json::from_slice::<CargoMessage>(line) else { continue };
        if message.reason != "compiler-artifact"
            || message.manifest_path.as_deref() != Some(manifest)
            || !message.target.is_some_and(|t| t.crate_types.iter().any(|c| c == "cdylib"))
        {
            continue;
        }
        artifacts.extend(message.filenames.into_iter().filter(|p| p.extension().is_some_and(|e| e == "wasm")));
    }
    artifacts.sort();
    artifacts.dedup();
    if artifacts.len() != 1 {
        bail!(
            "expected one WebAssembly cdylib from {}; point manifestPath at a package (not a virtual workspace) and set [lib] crate-type = [\"cdylib\"]",
            manifest.display()
        );
    }
    Ok(artifacts.remove(0))
}

async fn compile_module(project: &Project, module: &WasmConfig) -> Result<()> {
    let manifest = manifest_path(project, module)?;
    let output_dir = cache_dir(project, &project.out_dir().join("wasm"))?;
    let target_dir = cache_dir(project, &project.out_dir().join("wasm-target"))?;
    let mut command = Command::new("cargo");
    command
        .current_dir(manifest.parent().context("Cargo.toml has no parent directory")?)
        .args([
            "build",
            "--locked",
            "--lib",
            "--release",
            "--target",
            TARGET,
            "--message-format=json-render-diagnostics",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if !module.default_features {
        command.arg("--no-default-features");
    }
    if !module.features.is_empty() {
        command.arg("--features").arg(module.features.join(","));
    }
    let output = tokio::time::timeout(BUILD_TIMEOUT, command.output())
        .await
        .context("Cargo WASM build exceeded ten minutes; build the crate directly to diagnose it")?
        .with_context(|| format!("could not run cargo; install Rust and run `rustup target add {TARGET}`"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        bail!(
            "cargo build --locked failed ({}) for {}\n{}\nInstall the target with `rustup target add {TARGET}`. If Cargo.lock is missing or outdated, run `cargo generate-lockfile --manifest-path {}` and review/commit the lockfile.",
            output.status,
            module.manifest_path,
            stderr.trim(),
            module.manifest_path
        );
    }
    if !stderr.trim().is_empty() {
        eprintln!("wasm:{}: {}", module.name, stderr.trim());
    }
    let compiled = artifact(&output.stdout, &manifest)?.canonicalize().context("reading Cargo's WASM artifact")?;
    if !compiled.starts_with(&target_dir) {
        bail!("Cargo reported a WASM artifact outside the configured target directory");
    }
    let bytes = std::fs::read(&compiled).with_context(|| format!("reading {}", compiled.display()))?;
    if !bytes.starts_with(b"\0asm\x01\0\0\0") {
        bail!("Cargo did not produce a WebAssembly version 1 module");
    }
    write_artifact(&output_dir.join(format!("{}.wasm", module.name)), &bytes)
}

fn write_artifact(path: &Path, bytes: &[u8]) -> Result<()> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("WASM artifact must not be a symlink: {}", path.display());
    }
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    let suffix = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("wasm.{}.{suffix}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(bytes)?;
        drop(file);
        #[cfg(windows)]
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_artifacts_are_selected_by_manifest_and_crate_type_not_filename() {
        let manifest = Path::new("/project/rust/Cargo.toml");
        let output = concat!(
            "build-script progress\n",
            "{\"reason\":\"compiler-artifact\",\"manifest_path\":\"/dependency/Cargo.toml\",\"target\":{\"crate_types\":[\"cdylib\"]},\"filenames\":[\"/cache/dependency.wasm\"]}\n",
            "{\"reason\":\"compiler-artifact\",\"manifest_path\":\"/project/rust/Cargo.toml\",\"target\":{\"crate_types\":[\"cdylib\",\"rlib\"]},\"filenames\":[\"/cache/renamed.wasm\",\"/cache/renamed.rlib\"]}\n"
        );
        assert_eq!(artifact(output.as_bytes(), manifest).unwrap(), Path::new("/cache/renamed.wasm"));
        assert!(artifact(b"", manifest).unwrap_err().to_string().contains("cdylib"));
        assert!(artifact(output.as_bytes(), Path::new("/other/Cargo.toml")).is_err());
    }
}
