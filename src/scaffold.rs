//! `forge new`: project templates. Each template is an example under `examples/`,
//! embedded in the binary so the examples and the templates cannot drift apart.

use std::path::Path;

use anyhow::{Context, Result, bail};

macro_rules! files {
    (@source $dest:literal) => { $dest };
    (@source $dest:literal => $source:literal) => { $source };
    ($dir:literal: $($dest:literal $(=> $src:literal)?),* $(,)?) => {
        &[$(($dest, include_str!(concat!("../examples/", $dir, "/", files!(@source $dest $(=> $src)?))))),*]
    };
}

// The profiles share an editor, ABI and exporter. A generated project only
// carries the renderer implementations (and npm dependencies) it selects.
macro_rules! model_files {
    (with_rust, $($entries:tt)*) => {
        model_files!(
            "forge.config.json" => "profiles/rust/forge.config.json",
            "app/_model/rust-renderer.ts", "app/_model/orbit-input.ts", "rust/renderer/.gitignore", "rust/renderer/Cargo.toml",
            "rust/renderer/Cargo.lock", "rust/renderer/src/lib.rs", "rust/renderer/src/model.vert",
            "rust/renderer/src/model.frag", $($entries)*
        )
    };
    ($($entries:tt)*) => {
        files!("model-lab": "README.md", "tsconfig.json", "app/page.tsx", "app/layout.tsx", "app/model-lab.css",
            "app/_model/model.ts", "app/_model/glb.ts", "tools/mesh.py", "tools/mesh_glb.py",
            "rust/mesh/.gitignore", "rust/mesh/Cargo.toml", "rust/mesh/Cargo.lock", "rust/mesh/src/lib.rs",
            $($entries)*)
    };
}

type Files = &'static [(&'static str, &'static str)];

pub const TEMPLATES: &[(&str, &str, Files)] = &[
    (
        "react",
        "React + TypeScript: layouts, loaders, streaming SSR, API route",
        files!("react": "package.json", "forge.config.json", "tsconfig.json", "app/globals.css", "app/layout.tsx", "app/page.tsx", "app/_lib/posts.ts", "app/blog/loader.ts", "app/blog/page.tsx", "app/blog/[slug]/loader.ts", "app/blog/[slug]/page.tsx", "app/slow/page.tsx", "app/not-found.tsx", "app/api/hello/route.ts"),
    ),
    (
        "jsx",
        "React with plain JSX, persistent state, ships as one HTML file",
        files!("jsx": "package.json", "forge.config.json", "app/styles.css", "app/page.jsx"),
    ),
    (
        "vanilla",
        "no framework, no dependencies: render() + mount()",
        files!("vanilla": "package.json", "forge.config.json", "app/styles.css", "app/page.js", "app/about/page.js", "app/notes/[id]/page.js", "app/notes/[id]/loader.js", "app/not-found.js", "app/api/time/route.js", "app/_lib/notes.js"),
    ),
    (
        "three",
        "three.js game: fixed-step loop, input, GLSL imports, single HTML file",
        files!("three-game": "package.json", "forge.config.json", "app/game.css", "app/orb.frag", "app/orb.vert", "app/page.js"),
    ),
    (
        "models",
        "Rust/WASM procedural 3D models, WebGPU/WebGL2 viewer, GLB export, offline single HTML",
        model_files!(
            "forge.config.json",
            "package.json",
            "package-lock.json",
            "app/_model/view.ts",
            "app/_model/three-view.ts",
        ),
    ),
    (
        "models-rust",
        "Rust/WASM mesh generator and WebGL2 renderer, Python/OBJ import, offline or hosted",
        model_files!(with_rust,
            "package.json" => "profiles/rust/package.json",
            "package-lock.json" => "profiles/rust/package-lock.json",
            "app/_model/view.ts" => "app/_model/rust-view.ts",
        ),
    ),
    (
        "models-mixed",
        "Three.js or Rust/WASM renderer in one editor, with shared mesh import and GLB export",
        model_files!(with_rust,
            "package.json",
            "package-lock.json",
            "app/_model/view.ts" => "app/_model/mixed-view.ts",
            "app/_model/three-view.ts",
            "app/_model/rust-view.ts",
        ),
    ),
    (
        "ai",
        "streaming AI chat: OpenAI-compatible proxy (or offline echo) + React UI",
        files!("ai-chat": "package.json", "forge.config.json", "tsconfig.json", "app/chat.css", "app/page.tsx", "app/api/chat/route.ts"),
    ),
    (
        "starter",
        "app starter on forge:ui: sidebar shell, theme, data table, settings form, command palette, API route",
        files!("starter": "package.json", "forge.config.json", "tsconfig.json", "app/starter.css", "app/layout.tsx", "app/page.tsx", "app/loader.ts",
            "app/_data.ts", "app/_data.test.ts", "app/settings/page.tsx", "app/api/projects/route.ts"),
    ),
    (
        "pixel",
        "pixel-art editor in one HTML file that saves itself (forge:file: save-self, drop, export)",
        files!("pixel": "package.json", "forge.config.json", "app/pixel.css", "app/page.tsx"),
    ),
    (
        "arena",
        "multiplayer three.js arena: socket rooms, neural bots trained in a worker, offline single file",
        files!("arena": "package.json", "forge.config.json", "tsconfig.json", "app/arena.css", "app/layout.tsx", "app/page.tsx", "app/loader.ts", "app/not-found.tsx",
            "app/play/[room]/page.tsx", "app/lab/page.tsx", "app/api/room/[room]/route.ts", "app/api/lobby/route.ts", "app/api/champion/route.ts",
            "app/_game/sim.ts", "app/_game/brain.ts", "app/_game/room.ts", "app/_game/commentary.ts", "app/_game/registry.ts", "app/_game/client.ts",
            "app/_game/view.ts", "app/_game/preview.ts", "app/_game/evolve.worker.ts", "app/_game/floor.frag", "app/_game/floor.vert"),
    ),
];

const GITIGNORE: &str = "node_modules\n.forge\ndist\ntarget\n.env\n";

/// Written to every new project as `AGENTS.md`, so a coding agent that opens the project
/// knows the framework without searching for it.
const AGENT_GUIDE: &str = include_str!("../docs/AGENT_GUIDE.md");

/// Registers `forge mcp` for clients that read project-level MCP settings.
const MCP_CONFIG: &str =
    "{\n  \"mcpServers\": {\n    \"forge\": { \"command\": \"forge\", \"args\": [\"mcp\"] }\n  }\n}\n";

pub fn create(dir: &Path, template: &str) -> Result<()> {
    let Some((_, _, files)) = TEMPLATES.iter().find(|(name, _, _)| *name == template) else {
        let names: Vec<&str> = TEMPLATES.iter().map(|(n, _, _)| *n).collect();
        bail!("unknown template `{template}` (available: {})", names.join(", "));
    };
    if dir.exists() && std::fs::read_dir(dir)?.next().is_some() {
        bail!("{} already exists and is not empty", dir.display());
    }
    for (rel, contents) in files.iter() {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
    }
    std::fs::write(dir.join(".gitignore"), GITIGNORE)?;
    std::fs::write(dir.join("AGENTS.md"), AGENT_GUIDE)?;
    std::fs::write(dir.join(".mcp.json"), MCP_CONFIG)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_profiles_embed_the_selected_renderers_and_matching_dependencies() {
        use std::collections::BTreeMap;

        for (name, has_three, has_rust) in
            [("models", true, false), ("models-rust", false, true), ("models-mixed", true, true)]
        {
            let entries = TEMPLATES.iter().find(|t| t.0 == name).unwrap().2;
            let files: BTreeMap<_, _> = entries.iter().copied().collect();
            assert_eq!(files.len(), entries.len(), "{name} has conflicting destination paths");
            assert!(files.contains_key("tools/mesh.py"));
            assert!(files.contains_key("tools/mesh_glb.py"));
            assert!(files.contains_key("app/_model/view.ts"));
            assert_eq!(files.contains_key("app/_model/three-view.ts"), has_three);
            assert_eq!(files.contains_key("rust/renderer/Cargo.toml"), has_rust);
            let lock: serde_json::Value = serde_json::from_str(files["package-lock.json"]).unwrap();
            assert_eq!(lock["packages"].get("node_modules/three").is_some(), has_three);
            let config: serde_json::Value = serde_json::from_str(files["forge.config.json"]).unwrap();
            let modules = config["build"]["wasm"].as_array().unwrap();
            assert!(modules.iter().any(|module| module["name"] == "mesh"));
            assert_eq!(modules.iter().any(|module| module["name"] == "renderer"), has_rust);
            for module in modules {
                assert!(files.contains_key(module["manifestPath"].as_str().unwrap()));
            }
        }
    }

    #[test]
    fn every_template_scaffolds_a_valid_route_table() {
        for (name, _, _) in TEMPLATES {
            let dir = std::env::temp_dir().join(format!("forge-new-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            create(&dir, name).unwrap();
            let project =
                crate::config::Project::load(&dir, crate::config::Target::Server, crate::config::Mode::Prod).unwrap();
            let plan = crate::build::plan(&project).unwrap_or_else(|e| panic!("{name}: {e:#}"));
            assert!(!plan.routes.is_empty(), "{name}");
            assert!(std::fs::read_to_string(dir.join("AGENTS.md")).unwrap().contains("forge check"));
            let mcp: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join(".mcp.json")).unwrap()).unwrap();
            assert_eq!(mcp["mcpServers"]["forge"]["command"], "forge");
            assert!(create(&dir, name).is_err(), "refuses a non-empty directory");
            std::fs::remove_dir_all(&dir).unwrap();
        }
        assert!(create(Path::new("/nonexistent/x"), "nope").is_err());
    }
}
