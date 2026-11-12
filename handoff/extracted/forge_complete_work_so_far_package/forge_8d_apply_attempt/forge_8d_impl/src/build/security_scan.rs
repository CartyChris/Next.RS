use std::path::{Path, PathBuf};

use thiserror::Error;

use super::BuildSecurityConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityFinding {
    pub path: PathBuf,
    pub code: SecurityFindingCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityFindingCode {
    Eval,
    NewFunction,
    RemoteImport,
    InternalForgeImport,
    DangerousBuiltin,
}

#[derive(Debug, Error)]
pub enum SecurityScanError {
    #[error("security policy violation in `{path}`: {message}")]
    Violation { path: PathBuf, message: String },

    #[error("failed to read `{path}`: {source}")]
    Read { path: PathBuf, source: std::io::Error },
}

pub fn scan_user_source_files(
    files: &[PathBuf],
    config: &BuildSecurityConfig,
) -> Result<Vec<SecurityFinding>, SecurityScanError> {
    let mut findings = Vec::new();

    for path in files {
        let source = std::fs::read_to_string(path).map_err(|source| SecurityScanError::Read {
            path: path.clone(),
            source,
        })?;

        scan_user_source(path, &source, config, &mut findings)?;
    }

    if let Some(finding) = findings.first() {
        return Err(SecurityScanError::Violation {
            path: finding.path.clone(),
            message: finding.message.clone(),
        });
    }

    Ok(findings)
}

pub fn scan_user_source(
    path: &Path,
    source: &str,
    config: &BuildSecurityConfig,
    findings: &mut Vec<SecurityFinding>,
) -> Result<(), SecurityScanError> {
    if config.deny_eval && source.contains("eval(") {
        findings.push(SecurityFinding {
            path: path.to_path_buf(),
            code: SecurityFindingCode::Eval,
            message: "eval() is denied by Forge security policy".to_string(),
        });
    }

    if config.deny_new_function && source.contains("new Function(") {
        findings.push(SecurityFinding {
            path: path.to_path_buf(),
            code: SecurityFindingCode::NewFunction,
            message: "new Function() is denied by Forge security policy".to_string(),
        });
    }

    if config.deny_remote_imports
        && (source.contains("import(\"http://")
            || source.contains("import(\"https://")
            || source.contains("import(\"data:")
            || source.contains("from \"http://")
            || source.contains("from \"https://")
            || source.contains("from \"data:"))
    {
        findings.push(SecurityFinding {
            path: path.to_path_buf(),
            code: SecurityFindingCode::RemoteImport,
            message: "remote imports are denied by Forge security policy".to_string(),
        });
    }

    for denied in ["ext:forge_", "forge:internal"] {
        if source.contains(denied) {
            findings.push(SecurityFinding {
                path: path.to_path_buf(),
                code: SecurityFindingCode::InternalForgeImport,
                message: format!("internal Forge import `{denied}` is denied"),
            });
        }
    }

    Ok(())
}
