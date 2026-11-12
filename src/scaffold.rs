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

pub const TEMPLATES: &[(&str, &str, Files)] = &[(
    "vanilla",
    "no framework, no dependencies: render() + mount()",
    files!("vanilla": "package.json", "forge.config.json", "app/styles.css", "app/page.js", "app/about/page.js", "app/notes/[id]/page.js", "app/notes/[id]/loader.js", "app/not-found.js", "app/api/time/route.js", "app/_lib/notes.js"),
)];

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
