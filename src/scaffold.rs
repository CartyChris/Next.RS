//! `forge new`: project templates. Each template is an example under `examples/`,
//! embedded in the binary so the examples and the templates cannot drift apart.

use std::path::Path;

use anyhow::{Context, Result, bail};

macro_rules! files {
    ($dir:literal: $($file:literal),* $(,)?) => {
        &[$(($file, include_str!(concat!("../examples/", $dir, "/", $file)))),*]
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
        "ai",
        "streaming AI chat: OpenAI-compatible proxy (or offline echo) + React UI",
        files!("ai-chat": "package.json", "forge.config.json", "tsconfig.json", "app/chat.css", "app/page.tsx", "app/api/chat/route.ts"),
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

const GITIGNORE: &str = "node_modules\n.forge\ndist\n.env\n";

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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(create(&dir, name).is_err(), "refuses a non-empty directory");
            std::fs::remove_dir_all(&dir).unwrap();
        }
        assert!(create(Path::new("/nonexistent/x"), "nope").is_err());
    }
}
