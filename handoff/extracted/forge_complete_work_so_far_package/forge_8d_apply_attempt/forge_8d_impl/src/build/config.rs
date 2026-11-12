use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BundlerKind {
    Rolldown,
    LegacyForge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuildTarget {
    Server,
    Static,
    SingleHtml,
    WasmHost,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuildMode {
    Development,
    Production,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSecurityConfig {
    pub deny_eval: bool,
    pub deny_new_function: bool,
    pub deny_wasm_eval: bool,
    pub deny_remote_imports: bool,
}

impl BuildSecurityConfig {
    pub fn secure_default() -> Self {
        Self {
            deny_eval: true,
            deny_new_function: true,
            deny_wasm_eval: false,
            deny_remote_imports: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub project_root: PathBuf,
    pub route_root: PathBuf,
    pub output_dir: PathBuf,
    pub static_dir_name: String,
    pub mode: BuildMode,
    pub bundler: BundlerKind,
    pub target: BuildTarget,
    pub security: BuildSecurityConfig,
}

impl BuildConfig {
    pub fn from_project_root(project_root: impl Into<PathBuf>) -> Self {
        let project_root = project_root.into();

        Self {
            route_root: project_root.join("app"),
            output_dir: project_root.join(".forge"),
            static_dir_name: "static".to_string(),
            project_root,
            mode: BuildMode::Development,
            bundler: BundlerKind::Rolldown,
            target: BuildTarget::Server,
            security: BuildSecurityConfig::secure_default(),
        }
    }

    pub fn production(mut self) -> Self {
        self.mode = BuildMode::Production;
        self
    }
}
