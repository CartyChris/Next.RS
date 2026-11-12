use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{BuildTarget, BundleStats, BundleOutputKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ManifestBuildTarget {
    Server,
    Static,
    SingleHtml,
    WasmHost,
}

pub fn manifest_target(target: &BuildTarget) -> ManifestBuildTarget {
    match target {
        BuildTarget::Server => ManifestBuildTarget::Server,
        BuildTarget::Static => ManifestBuildTarget::Static,
        BuildTarget::SingleHtml => ManifestBuildTarget::SingleHtml,
        BuildTarget::WasmHost => ManifestBuildTarget::WasmHost,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestCapabilities {
    pub ssr: bool,
    pub streaming_ssr: bool,
    pub route_data_endpoint: bool,
    pub server_loaders: bool,
    pub client_hydration: bool,
    pub client_navigation: bool,
    pub persistent_server_cache: bool,
    pub single_file: bool,
    pub wasm_host: bool,
}

impl ManifestCapabilities {
    pub fn for_target(target: &BuildTarget) -> Self {
        match target {
            BuildTarget::Server => Self {
                ssr: true,
                streaming_ssr: true,
                route_data_endpoint: true,
                server_loaders: true,
                client_hydration: true,
                client_navigation: true,
                persistent_server_cache: true,
                single_file: false,
                wasm_host: false,
            },
            BuildTarget::Static => Self {
                ssr: false,
                streaming_ssr: false,
                route_data_endpoint: false,
                server_loaders: false,
                client_hydration: true,
                client_navigation: true,
                persistent_server_cache: false,
                single_file: false,
                wasm_host: false,
            },
            BuildTarget::SingleHtml => Self {
                ssr: false,
                streaming_ssr: false,
                route_data_endpoint: false,
                server_loaders: false,
                client_hydration: true,
                client_navigation: true,
                persistent_server_cache: false,
                single_file: true,
                wasm_host: false,
            },
            BuildTarget::WasmHost => Self {
                ssr: false,
                streaming_ssr: false,
                route_data_endpoint: false,
                server_loaders: false,
                client_hydration: true,
                client_navigation: true,
                persistent_server_cache: false,
                single_file: false,
                wasm_host: true,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestBundleFile {
    pub path: PathBuf,
    pub public_path: String,
    pub kind: ManifestBundleFileKind,
    pub hash: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ManifestBundleFileKind {
    JavaScript,
    Css,
    Asset,
    SourceMap,
    Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestBundle {
    pub bundler: String,
    pub target: ManifestBuildTarget,
    pub files: Vec<ManifestBundleFile>,
}

pub fn stats_to_manifest_files(stats: &BundleStats) -> Vec<ManifestBundleFile> {
    let mut files = Vec::new();

    for output in &stats.outputs {
        for file in &output.files {
            files.push(ManifestBundleFile {
                path: file.path.clone(),
                public_path: file.public_path.clone(),
                kind: match file.kind {
                    BundleOutputKind::JavaScript => ManifestBundleFileKind::JavaScript,
                    BundleOutputKind::Css => ManifestBundleFileKind::Css,
                    BundleOutputKind::Asset => ManifestBundleFileKind::Asset,
                    BundleOutputKind::SourceMap => ManifestBundleFileKind::SourceMap,
                    BundleOutputKind::Manifest => ManifestBundleFileKind::Manifest,
                },
                hash: file.hash.clone(),
                bytes: file.bytes,
            });
        }
    }

    files
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientAsset {
    pub public_path: String,
    pub disk_path: PathBuf,
    pub integrity_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetManifest {
    pub manifest_version: u32,
    pub framework_version: String,
    pub build_target: ManifestBuildTarget,
    pub capabilities: ManifestCapabilities,
    pub bundle: ManifestBundle,
    pub server_entry: PathBuf,
    pub client_entry: ClientAsset,
}
