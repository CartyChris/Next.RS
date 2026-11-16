//! `forge run` and `forge test`: run scripts and tests in the embedded engine.
//!
//! The file is bundled like server code (TypeScript, JSX, npm packages, `forge:*` modules,
//! the security scan) and executed in one worker with the project's sandbox settings:
//! `fetch` only reaches hosts in `server.allowFetch`, and `env` only has `server.env`.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{Side, build_pool, bundle, generated, spec};
use crate::config::Project;

#[derive(Debug, Deserialize)]
pub struct TestResult {
    pub file: String,
    pub name: String,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub skipped: bool,
    #[serde(default)]
    pub ms: f64,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub stack: String,
}

#[derive(Debug, Deserialize)]
struct Outcome {
    ok: bool,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    tests: Vec<TestResult>,
}

const PRELUDE: &str = r#"import { host } from "./runtime/host-ref.js";
const finish = (id, outcome) => {
  host.write(id, JSON.stringify(outcome));
  host.end(id);
};
// The engine's stacks do not repeat the message.
const failure = (e) => ({ ok: false, error: e instanceof Error ? `${e.name}: ${e.message}\n${e.stack ?? ""}`.trimEnd() : String(e) });
"#;

fn import_path(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).expect("string")
}

/// Bundle `entry` (generated code) and run its dispatch once.
async fn execute(project: &Project, entry: String) -> Result<Outcome> {
    let dir = project.generated_dir();
    generated::write_runtime(&dir)?;
    super::write_types(project)?;
    let entry_path = dir.join("run-entry.js");
    generated::write_if_changed(&entry_path, &entry)?;
    let bundled = bundle::bundle(spec(project, ("server", &entry_path), Side::Server)).await?;
    let code: Arc<str> = bundled.entry("server").context("the script produced no code")?.code.as_str().into();
    let pool = build_pool(project, code, 1).await?;
    let reply = pool.dispatch("{}".to_string(), None).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let body = reply.body.collect().await;
    serde_json::from_slice(&body)
        .with_context(|| format!("the engine stopped early: {}", String::from_utf8_lossy(&body)))
}

/// Run one script. If it default-exports a function, that is called and its result returned
/// (strings as they are, other values as JSON).
pub async fn run(project: &Project, file: &Path) -> Result<Option<String>> {
    let file = file.canonicalize().with_context(|| format!("{} not found", file.display()))?;
    let entry = format!(
        r#"{PRELUDE}globalThis.__forge_dispatch = (id) => {{
  host.head(id, JSON.stringify({{ status: 200, headers: [] }}));
  (async () => {{
    const mod = await import({});
    let value = typeof mod.default === "function" ? await mod.default() : undefined;
    if (value !== undefined && typeof value !== "string") value = JSON.stringify(value, null, 2);
    return {{ ok: true, value }};
  }})().catch(failure).then((outcome) => finish(id, outcome));
}};
"#,
        import_path(&file)
    );
    let outcome = execute(project, entry).await?;
    if !outcome.ok {
        bail!("{}", outcome.error.unwrap_or_else(|| "the script failed".into()));
    }
    Ok(outcome.value)
}

const SKIP_DIRS: &[&str] = &["node_modules", "dist", "target"];

fn is_test(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mts" | "mjs") && (stem.ends_with(".test") || stem.ends_with(".spec"))
}

/// Test files under `root` (`*.test.*`, `*.spec.*`), sorted; `filter` keeps paths containing it.
pub fn find_tests(root: &Path, filter: Option<&str>) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if path.is_dir() {
                if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                    walk(&path, out);
                }
            } else if is_test(&name) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out.retain(|p| filter.is_none_or(|f| p.strip_prefix(root).unwrap_or(p).to_string_lossy().contains(f)));
    out
}

/// Run test files and return every test's result, in order.
pub async fn test(project: &Project, files: &[PathBuf]) -> Result<Vec<TestResult>> {
    let mut body = String::new();
    for file in files {
        let label = file.strip_prefix(&project.root).unwrap_or(file).to_string_lossy().replace('\\', "/");
        body.push_str(&format!(
            "    try {{ await import({}); tests.push(...(await __run({}))); }} catch (e) {{ tests.push({{ file: {}, name: \"(loading the file)\", ok: false, error: `${{e?.name ?? \"Error\"}}: ${{e?.message ?? e}}`, stack: \"\" }}); }}\n",
            import_path(file),
            serde_json::to_string(&label)?,
            serde_json::to_string(&label)?,
        ));
    }
    let entry = format!(
        r#"{PRELUDE}import {{ __run }} from "forge:test";
globalThis.__forge_dispatch = (id) => {{
  host.head(id, JSON.stringify({{ status: 200, headers: [] }}));
  (async () => {{
    const tests = [];
{body}    return {{ ok: true, tests }};
  }})().catch(failure).then((outcome) => finish(id, outcome));
}};
"#
    );
    let outcome = execute(project, entry).await?;
    if !outcome.ok {
        bail!("{}", outcome.error.unwrap_or_else(|| "the test run failed".into()));
    }
    Ok(outcome.tests)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_names() {
        for yes in ["a.test.ts", "cart.spec.tsx", "x.test.js", "deep.name.test.mjs"] {
            assert!(is_test(yes), "{yes}");
        }
        for no in ["test.ts", "a.tests.ts", "a.test.css", "contest.ts", "a.test"] {
            assert!(!is_test(no), "{no}");
        }
    }
}
