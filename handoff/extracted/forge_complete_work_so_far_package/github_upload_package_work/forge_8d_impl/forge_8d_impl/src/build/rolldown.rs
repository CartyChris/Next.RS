use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use tokio::process::Command;

use crate::build::{
    BundleEntry, BundleOptions, BundleOutput, BundleOutputFile, BundleOutputKind, BundleStats,
    BundleTarget, BundlerBackend, BundlerError,
};

#[derive(Debug, Clone)]
pub struct RolldownBundler {
    pub executable: String,
}

impl Default for RolldownBundler {
    fn default() -> Self {
        Self {
            executable: "rolldown".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl BundlerBackend for RolldownBundler {
    async fn bundle(
        &self,
        entries: Vec<BundleEntry>,
        options: BundleOptions,
    ) -> Result<BundleStats, BundlerError> {
        let config_path = options.output_dir.join("rolldown.config.mjs");
        let manifest_path = options.output_dir.join("rolldown-manifest.json");
        let config = generate_rolldown_config(&entries, &options, &manifest_path);

        write_file(&config_path, config)?;

        let output = Command::new(&self.executable)
            .arg("-c")
            .arg(&config_path)
            .current_dir(&options.project_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|source| BundlerError::CommandSpawn {
                program: self.executable.clone(),
                source,
            })?;

        if !output.status.success() {
            return Err(BundlerError::CommandFailed {
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        collect_bundle_stats(&options.output_dir, &options.public_base, &entries)
    }
}

pub fn generate_rolldown_config(
    entries: &[BundleEntry],
    options: &BundleOptions,
    _manifest_path: &Path,
) -> String {
    let server_entries = entries
        .iter()
        .filter(|entry| matches!(entry.target, BundleTarget::Server))
        .collect::<Vec<_>>();

    let browser_entries = entries
        .iter()
        .filter(|entry| matches!(entry.target, BundleTarget::Browser))
        .collect::<Vec<_>>();

    let mut configs = Vec::new();

    if !server_entries.is_empty() {
        configs.push(generate_single_rolldown_config(
            "server",
            &server_entries,
            &options.output_dir.join("server"),
            "node",
            "esm",
            options,
        ));
    }

    if !browser_entries.is_empty() {
        configs.push(generate_single_rolldown_config(
            "browser",
            &browser_entries,
            &options.output_dir.join("static"),
            "browser",
            "esm",
            options,
        ));
    }

    let configs = configs.join(",\n");

    format!(
        r#"
import {{ defineConfig }} from "rolldown";

function forgeAliasPlugin(aliases) {{
  return {{
    name: "forge-aliases",
    resolveId(source) {{
      if (aliases[source]) {{
        return aliases[source];
      }}
      return null;
    }}
  }};
}}

function forgeSecurityPlugin(options) {{
  const forbiddenPrefixes = ["ext:forge_", "forge:internal"];

  return {{
    name: "forge-security-policy",
    resolveId(source, importer) {{
      if (forbiddenPrefixes.some((prefix) => source.startsWith(prefix))) {{
        throw new Error(`Forge security policy denied internal import ${{source}} from ${{importer}}`);
      }}

      if (options.denyRemoteImports && /^(https?:|data:|blob:)/.test(source)) {{
        throw new Error(`Forge security policy denied remote import ${{source}}`);
      }}

      return null;
    }},
    transform(code, id) {{
      if (options.denyEval && /\beval\s*\(/.test(code)) {{
        throw new Error(`Forge security policy denied eval() in ${{id}}`);
      }}
      if (options.denyNewFunction && /\bnew\s+Function\s*\(/.test(code)) {{
        throw new Error(`Forge security policy denied new Function() in ${{id}}`);
      }}
      if (options.denyWasmEval && /WebAssembly\.(compile|instantiate)\s*\(/.test(code)) {{
        throw new Error(`Forge security policy denied dynamic WebAssembly in ${{id}}`);
      }}
      return null;
    }}
  }};
}}

function forgeManifestPlugin(fileName) {{
  return {{
    name: "forge-manifest",
    generateBundle(_options, bundle) {{
      const manifest = {{}};

      for (const [bundleFileName, chunk] of Object.entries(bundle)) {{
        if (chunk.type === "chunk") {{
          manifest[chunk.name] = {{
            fileName: bundleFileName,
            imports: chunk.imports,
            dynamicImports: chunk.dynamicImports,
            isEntry: chunk.isEntry,
            facadeModuleId: chunk.facadeModuleId
          }};
        }} else {{
          manifest[bundleFileName] = {{
            fileName: bundleFileName,
            type: "asset"
          }};
        }}
      }}

      this.emitFile({{
        type: "asset",
        fileName,
        source: JSON.stringify(manifest, null, 2)
      }});
    }}
  }};
}}

export default defineConfig([
{configs}
]);
"#
    )
}

fn generate_single_rolldown_config(
    name: &str,
    entries: &[&BundleEntry],
    out_dir: &Path,
    platform: &str,
    format: &str,
    options: &BundleOptions,
) -> String {
    let input = entries
        .iter()
        .map(|entry| {
            format!(
                "{}: {}",
                serde_json::to_string(&entry.name).expect("entry name serializes"),
                serde_json::to_string(&entry.input.display().to_string())
                    .expect("entry input serializes")
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let out_dir_json = serde_json::to_string(&out_dir.display().to_string())
        .expect("out_dir serializes");

    let sourcemap = if options.sourcemap { "true" } else { "false" };
    let minify = if options.minify { "true" } else { "false" };
    let splitting = if options.splitting { "true" } else { "false" };
    let node_env = if options.development { "\"development\"" } else { "\"production\"" };
    let manifest_name = format!("forge-{name}-bundle-manifest.json");

    let forge_data = options.output_dir.join("generated/internals/forge-data-runtime.js");
    let forge_cache = options.output_dir.join("generated/internals/forge-cache-runtime.js");
    let forge_navigation = options.output_dir.join("generated/internals/forge-navigation-runtime.js");

    format!(
        r#"
{{
  input: {{
{input}
  }},
  platform: {platform:?},
  output: {{
    dir: {out_dir_json},
    format: {format:?},
    sourcemap: {sourcemap},
    entryFileNames: "[name].[hash].js",
    chunkFileNames: "chunks/[name].[hash].js",
    assetFileNames: "assets/[name].[hash][extname]",
    codeSplitting: {splitting}
  }},
  minify: {minify},
  treeshake: true,
  transform: {{
    jsx: {{
      runtime: "automatic",
      importSource: "react"
    }},
    define: {{
      "process.env.NODE_ENV": {node_env}
    }}
  }},
  plugins: [
    forgeAliasPlugin({{
      "forge:data": {},
      "forge:cache": {},
      "forge:navigation": {}
    }}),
    forgeSecurityPlugin({{
      denyEval: {},
      denyNewFunction: {},
      denyWasmEval: {},
      denyRemoteImports: {}
    }}),
    forgeManifestPlugin({manifest_name:?})
  ]
}}
"#,
        serde_json::to_string(&forge_data.display().to_string()).unwrap(),
        serde_json::to_string(&forge_cache.display().to_string()).unwrap(),
        serde_json::to_string(&forge_navigation.display().to_string()).unwrap(),
        options.security.deny_eval,
        options.security.deny_new_function,
        options.security.deny_wasm_eval,
        options.security.deny_remote_imports,
    )
}

pub fn collect_bundle_stats(
    output_dir: &Path,
    public_base: &str,
    entries: &[BundleEntry],
) -> Result<BundleStats, BundlerError> {
    let mut outputs = Vec::new();

    for entry in entries {
        let dir = match entry.target {
            BundleTarget::Server => output_dir.join("server"),
            BundleTarget::Browser => output_dir.join("static"),
            BundleTarget::Worker => output_dir.join("workers"),
            BundleTarget::SingleFileHtml => output_dir.join("single-html"),
            BundleTarget::WasmHost => output_dir.join("wasm-host"),
        };

        let files = collect_output_files(&dir, public_base)?;

        outputs.push(BundleOutput {
            entry_name: entry.name.clone(),
            files,
        });
    }

    let mut total_bytes = 0;
    let mut javascript_bytes = 0;
    let mut css_bytes = 0;
    let mut asset_bytes = 0;

    for output in &outputs {
        for file in &output.files {
            total_bytes += file.bytes;
            match file.kind {
                BundleOutputKind::JavaScript => javascript_bytes += file.bytes,
                BundleOutputKind::Css => css_bytes += file.bytes,
                BundleOutputKind::Asset => asset_bytes += file.bytes,
                BundleOutputKind::SourceMap | BundleOutputKind::Manifest => {}
            }
        }
    }

    Ok(BundleStats {
        outputs,
        total_bytes,
        javascript_bytes,
        css_bytes,
        asset_bytes,
    })
}

pub fn collect_output_files(
    dir: &Path,
    public_base: &str,
) -> Result<Vec<BundleOutputFile>, BundlerError> {
    let mut files = Vec::new();

    if !dir.exists() {
        return Ok(files);
    }

    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry.map_err(|err| BundlerError::CommandFailed {
            status: "walkdir".to_string(),
            stderr: err.to_string(),
        })?;

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path().to_path_buf();
        let bytes = std::fs::read(&path).map_err(|source| BundlerError::CreateInput {
            path: path.clone(),
            source,
        })?;

        let relative = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let hash = blake3::hash(&bytes).to_hex().to_string();

        files.push(BundleOutputFile {
            public_path: format!("{}/{}", public_base.trim_end_matches('/'), relative),
            kind: classify_output_file(&path),
            path,
            hash,
            bytes: bytes.len() as u64,
        });
    }

    Ok(files)
}

pub fn classify_output_file(path: &Path) -> BundleOutputKind {
    match path.extension().and_then(|value| value.to_str()) {
        Some("js") | Some("mjs") => BundleOutputKind::JavaScript,
        Some("css") => BundleOutputKind::Css,
        Some("map") => BundleOutputKind::SourceMap,
        Some("json") => BundleOutputKind::Manifest,
        _ => BundleOutputKind::Asset,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolldownManifestEntry {
    pub file_name: String,
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default)]
    pub dynamic_imports: Vec<String>,
    #[serde(default)]
    pub is_entry: bool,
    #[serde(default)]
    pub facade_module_id: Option<String>,
}

pub type RolldownManifest = BTreeMap<String, RolldownManifestEntry>;

pub fn find_rolldown_entry<'a>(
    manifest: &'a RolldownManifest,
    entry_name: &str,
) -> Option<&'a RolldownManifestEntry> {
    manifest.get(entry_name)
}

fn write_file(path: &Path, contents: String) -> Result<(), BundlerError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| BundlerError::CreateInput {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    std::fs::write(path, contents).map_err(|source| BundlerError::WriteConfig {
        path: path.to_path_buf(),
        source,
    })
}
