use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::BuildTarget;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BundleTarget {
    Server,
    Browser,
    Worker,
    SingleFileHtml,
    WasmHost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleEntry {
    pub name: String,
    pub input: PathBuf,
    pub target: BundleTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleOptions {
    pub project_root: PathBuf,
    pub output_dir: PathBuf,
    pub public_base: String,
    pub development: bool,
    pub minify: bool,
    pub sourcemap: bool,
    pub splitting: bool,
    pub target: BuildTarget,
    pub security: BundleSecurityOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleSecurityOptions {
    pub deny_eval: bool,
    pub deny_new_function: bool,
    pub deny_wasm_eval: bool,
    pub deny_remote_imports: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleOutput {
    pub entry_name: String,
    pub files: Vec<BundleOutputFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleOutputFile {
    pub path: PathBuf,
    pub public_path: String,
    pub kind: BundleOutputKind,
    pub hash: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BundleOutputKind {
    JavaScript,
    Css,
    Asset,
    SourceMap,
    Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleStats {
    pub outputs: Vec<BundleOutput>,
    pub total_bytes: u64,
    pub javascript_bytes: u64,
    pub css_bytes: u64,
    pub asset_bytes: u64,
}

#[derive(Debug, Error)]
pub enum BundlerError {
    #[error("failed to create bundler input `{path}`: {source}")]
    CreateInput { path: PathBuf, source: std::io::Error },

    #[error("failed to write bundler config `{path}`: {source}")]
    WriteConfig { path: PathBuf, source: std::io::Error },

    #[error("failed to execute bundler command `{program}`: {source}")]
    CommandSpawn { program: String, source: std::io::Error },

    #[error("bundler command failed with status {status}: {stderr}")]
    CommandFailed { status: String, stderr: String },

    #[error("failed to parse bundler output manifest `{path}`: {source}")]
    ParseManifest { path: PathBuf, source: serde_json::Error },

    #[error("bundler output missing expected entry `{0}`")]
    MissingEntry(String),

    #[error("unsupported target for backend: {0:?}")]
    UnsupportedTarget(BundleTarget),
}

#[async_trait]
pub trait BundlerBackend: Send + Sync {
    async fn bundle(
        &self,
        entries: Vec<BundleEntry>,
        options: BundleOptions,
    ) -> Result<BundleStats, BundlerError>;
}
