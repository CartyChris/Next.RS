//! `forge mcp`: a Model Context Protocol server over stdio, so coding agents can read the
//! documentation that matches this binary and inspect, check, build and pack projects.
//!
//! The protocol is JSON-RPC 2.0, one message per line. Everything here is local: the docs
//! are embedded in the binary and the tools run the same code as the CLI commands.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::{
    build::{self, generated::UI_DTS},
    config::{Mode, Project, Target},
    scaffold,
};

/// Protocol revisions this server speaks, newest first.
const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Documentation shipped inside the binary: `(name, summary, markdown)`.
pub const DOCS: &[(&str, &str, &str)] = &[
    ("agents", "Start here: commands, conventions and pitfalls for agents", include_str!("../AGENTS.md")),
    (
        "guide",
        "Framework guide: routing, pages, layouts, loaders, API routes, sockets, targets, modules, configuration",
        include_str!("../docs/GUIDE.md"),
    ),
    ("ui", "forge:ui component reference, theming and patterns", include_str!("../docs/FORGE_UI.md")),
    ("design", "Design rules: how to build interfaces that look professional", include_str!("../docs/DESIGN.md")),
    ("architecture", "How the build, runtime and server work", include_str!("../docs/ARCHITECTURE.md")),
];

/// One heading and the text under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub doc: &'static str,
    /// Heading trail, e.g. `Build targets › Self-saving single files`.
    pub heading: String,
    pub body: String,
}

/// Split markdown at `##` and `###` headings (ignoring fenced code).
pub fn sections(doc: &'static str, markdown: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut h2 = String::new();
    let mut current = Section { doc, heading: "Overview".into(), body: String::new() };
    let mut fenced = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        let heading = (!fenced)
            .then(|| line.strip_prefix("## ").map(|h| (2, h)).or(line.strip_prefix("### ").map(|h| (3, h))))
            .flatten();
        match heading {
            Some((level, text)) => {
                if !current.body.trim().is_empty() {
                    out.push(current.clone());
                }
                let text = text.trim().to_string();
                let heading = if level == 2 || h2.is_empty() { text.clone() } else { format!("{h2} › {text}") };
                if level == 2 {
                    h2 = text;
                }
                current = Section { doc, heading, body: String::new() };
            }
            None => {
                current.body.push_str(line);
                current.body.push('\n');
            }
        }
    }
    if !current.body.trim().is_empty() {
        out.push(current);
    }
    out
}

fn all_sections(only: Option<&str>) -> Vec<Section> {
    DOCS.iter()
        .filter(|(name, _, _)| only.is_none_or(|o| o == *name))
        .flat_map(|(name, _, md)| sections(name, md))
        .collect()
}

/// Rank sections for a query: words in the heading count most, then occurrences in the text.
pub fn search(query: &str, only: Option<&str>) -> Vec<(usize, Section)> {
    let words: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == ':' || c == '_' || c == '-' || c == '.'))
        .filter(|w| w.len() > 1)
        .map(String::from)
        .collect();
    let mut ranked: Vec<(usize, Section)> = all_sections(only)
        .into_iter()
        .filter_map(|s| {
            let (heading, body) = (s.heading.to_lowercase(), s.body.to_lowercase());
            let mut score = 0;
            let mut matched = 0;
            for w in &words {
                let hits = body.matches(w.as_str()).count().min(6) + if heading.contains(w.as_str()) { 8 } else { 0 };
                if hits > 0 {
                    matched += 1;
                }
                score += hits;
            }
            // Sections matching every word come before partial matches.
            (matched > 0).then(|| (score + if matched == words.len() { 100 } else { 0 }, s))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    ranked
}

/// Public names exported by `forge:ui`, from its generated declarations.
pub fn ui_exports() -> Vec<String> {
    let mut names: Vec<String> = UI_DTS
        .lines()
        .filter_map(|l| l.trim().strip_prefix("export function "))
        .filter_map(|l| l.split(['(', '<']).next())
        .map(String::from)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The declarations for one `forge:ui` export and its props, followed by the library types
/// they mention (two levels deep), so unions such as `ButtonVariant` are spelled out.
pub fn ui_declaration(name: &str) -> Option<String> {
    let mut text = ui_declaration_of(name)?;
    let types: Vec<&str> = UI_DTS
        .lines()
        .filter_map(|l| l.trim().strip_prefix("export type ").or_else(|| l.trim().strip_prefix("export interface ")))
        .filter_map(|l| l.split(|c: char| !(c.is_alphanumeric() || c == '_')).next())
        .collect();
    let mut seen = vec![name.to_string(), format!("{name}Props")];
    for _ in 0..2 {
        let mentioned: Vec<&str> = types
            .iter()
            .copied()
            .filter(|t| !seen.iter().any(|s| s == t))
            .filter(|t| text.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|word| word == *t))
            .collect();
        for t in mentioned {
            seen.push(t.to_string());
            if let Some(decl) = ui_declaration_of(t) {
                text.push_str("\n\n");
                text.push_str(&decl);
            }
        }
    }
    Some(text)
}

fn ui_declaration_of(name: &str) -> Option<String> {
    let lines: Vec<&str> = UI_DTS.lines().collect();
    let starts = [
        format!("export function {name}("),
        format!("export function {name}<"),
        format!("export interface {name}Props"),
        format!("export type {name}Props"),
        format!("export interface {name} "),
        format!("export type {name} "),
    ];
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if starts.iter().any(|s| trimmed.starts_with(s.as_str())) {
            // Include the doc comment directly above.
            let mut from = i;
            if from > 0 && lines[from - 1].trim() == "*/" {
                while from > 0 && !lines[from - 1].trim_start().starts_with("/**") {
                    from -= 1;
                }
                from = from.saturating_sub(1);
            } else if from > 0 && lines[from - 1].trim_start().starts_with("/**") {
                from -= 1;
            }
            let indent = lines[i].len() - trimmed.len();
            let mut to = i;
            let block = trimmed.ends_with('{');
            if block {
                while to + 1 < lines.len() && !(lines[to].starts_with(&format!("{}}}", " ".repeat(indent))) && to > i) {
                    to += 1;
                }
            } else {
                while to + 1 < lines.len() && !lines[to].trim_end().ends_with(';') {
                    to += 1;
                }
            }
            out.extend(lines[from..=to].iter().map(|l| l.get(4..).unwrap_or(l.trim_start()).to_string()));
            out.push(String::new());
            i = to;
        }
        i += 1;
    }
    (!out.is_empty()).then(|| out.join("\n").trim_end().to_string())
}

/// The theme catalog of `forge:ui`, read from the JSON block in its source.
pub fn ui_themes() -> Vec<Value> {
    let source = build::generated::UI_FILES.iter().find(|(name, _)| *name == "themes.ts").map_or("", |(_, s)| *s);
    let json = source
        .split("/* themes:start */")
        .nth(1)
        .and_then(|rest| rest.split("/* themes:end */").next())
        .unwrap_or("[]");
    serde_json::from_str(json).unwrap_or_default()
}

/// Themes whose name, keywords or description match the words of `query`, best first.
pub fn find_themes(query: &str) -> Vec<Value> {
    let words: Vec<String> =
        query.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 1).map(String::from).collect();
    let mut scored: Vec<(usize, Value)> = ui_themes()
        .into_iter()
        .filter_map(|theme| {
            let tags = format!(
                "{} {} {}",
                theme["name"].as_str()?,
                theme["label"].as_str()?,
                theme["keywords"].as_array()?.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")
            )
            .to_lowercase();
            let text = theme["description"].as_str()?.to_lowercase();
            let score: usize = words
                .iter()
                .map(|w| {
                    if tags.contains(w.as_str()) {
                        3
                    } else if text.contains(w.as_str()) {
                        1
                    } else {
                        0
                    }
                })
                .sum();
            (score > 0).then_some((score, theme))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, t)| t).collect()
}

// ---- tools ----

fn tools() -> Value {
    let root = json!({ "type": "string", "description": "Project directory (contains app/). Default: the server's working directory.", "default": "." });
    let read_only =
        json!({ "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false });
    let writes =
        json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false });
    json!([
        {
            "name": "forge_docs_search",
            "title": "Search the Next.RS docs",
            "description": "Search the documentation built into this forge binary (framework guide, forge:ui reference, design rules, agent notes, architecture) and return the best-matching sections as markdown. Use this before writing code for a feature you have not used yet, e.g. query \"loader redirect\", \"websocket room\", \"single-html\", \"Table sortable\".",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": "Words to look for, e.g. \"api route streaming\"" },
                "doc": { "type": "string", "enum": DOCS.iter().map(|d| d.0).collect::<Vec<_>>(), "description": "Limit the search to one document" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 10, "default": 3, "description": "Sections to return" },
                "offset": { "type": "integer", "minimum": 0, "default": 0, "description": "Skip this many results (for the next page)" }
            }, "required": ["query"] },
            "annotations": read_only
        },
        {
            "name": "forge_docs_read",
            "title": "Read a Next.RS doc",
            "description": "Read one built-in document. Without `section` it returns the document's outline (its headings); with `section` it returns that section's markdown. Documents: agents (start here), guide, ui, design, architecture.",
            "inputSchema": { "type": "object", "properties": {
                "doc": { "type": "string", "enum": DOCS.iter().map(|d| d.0).collect::<Vec<_>>() },
                "section": { "type": "string", "description": "A heading from the outline, e.g. \"Loaders\" (case-insensitive, partial match allowed)" }
            }, "required": ["doc"] },
            "annotations": read_only
        },
        {
            "name": "forge_ui_component",
            "title": "Look up a forge:ui component",
            "description": "Without `name`: list every export of the built-in forge:ui component library. With `name` (e.g. \"Table\", \"Theme\", \"useHotkey\"): return its TypeScript declaration and props with their doc comments, exactly as this binary ships them.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "An export of forge:ui, e.g. \"Button\"" }
            } },
            "annotations": read_only
        },
        {
            "name": "forge_ui_themes",
            "title": "Find a forge:ui theme",
            "description": "The catalog of named looks for forge:ui (`<Theme preset=\"…\">`): forge, liquid-glass, graphite, paper, clinic, midnight, terminal, arcade. Pass `query` with the words the user used (\"frosted Apple-style glass\", \"dense admin\", \"playful game menu\") to get the best matches, each with its settings and how to customize it further.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": "A look described in plain words. Omit to list every theme." }
            } },
            "annotations": read_only
        },
        {
            "name": "forge_templates",
            "title": "List project templates",
            "description": "List the templates `forge_new` can create, with what each one demonstrates.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": read_only
        },
        {
            "name": "forge_new",
            "title": "Create a project",
            "description": "Create a new Next.RS project from a template in an empty or missing directory. Afterwards run `npm install` in it (the vanilla template needs none), then `forge dev`.",
            "inputSchema": { "type": "object", "properties": {
                "dir": { "type": "string", "description": "Directory to create, e.g. \"my-app\"" },
                "template": { "type": "string", "enum": scaffold::TEMPLATES.iter().map(|t| t.0).collect::<Vec<_>>(), "default": "starter" }
            }, "required": ["dir"] },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false }
        },
        {
            "name": "forge_routes",
            "title": "List a project's routes",
            "description": "Scan a project's app/ directory and return its route table: URL pattern, kind (page or api), source file, renderer, and whether it has a loader, server rendering, a socket, or a not-found page. Fast; does not bundle.",
            "inputSchema": { "type": "object", "properties": { "root": root } },
            "annotations": read_only
        },
        {
            "name": "forge_check",
            "title": "Check a project",
            "description": "Validate routes, bundle the server code and run the security scan (eval, remote imports, internals) without writing a build. Also writes .forge/types so `tsc` can type-check forge:* imports. Returns errors with file:line:column, or the route table and warnings.",
            "inputSchema": { "type": "object", "properties": { "root": root } },
            "annotations": writes
        },
        {
            "name": "forge_build",
            "title": "Build a project",
            "description": "Build a project for one target: `server` (SSR app for `forge start`), `static` (prerendered site in dist/), or `single-html` (one offline dist/index.html). Returns output paths, sizes and warnings, or the build errors.",
            "inputSchema": { "type": "object", "properties": {
                "root": root,
                "target": { "type": "string", "enum": ["server", "static", "single-html"], "default": "server" }
            } },
            "annotations": writes
        },
        {
            "name": "forge_pack",
            "title": "Pack a page or component into one HTML file",
            "description": "Turn an .html page (CDN scripts, import maps, text/babel) or a single .jsx/.tsx/.js/.ts component into one offline HTML file. Imports install from npm; forge:ui is available. Needs network access the first time a package is used. Returns the output path and size.",
            "inputSchema": { "type": "object", "properties": {
                "input": { "type": "string", "description": "Path to the page or component file" },
                "output": { "type": "string", "description": "Output file. Default: next to the input" }
            }, "required": ["input"] },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": true }
        }
    ])
}

fn text_result(text: String, structured: Option<Value>) -> Value {
    let mut result = json!({ "content": [{ "type": "text", "text": text }] });
    if let Some(s) = structured {
        result["structuredContent"] = s;
    }
    result
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn project(args: &Value, target: Target, mode: Mode) -> Result<Project> {
    let root = PathBuf::from(arg_str(args, "root").unwrap_or("."));
    Project::load(&root, target, mode).with_context(|| {
        format!(
            "`{}` is not a Next.RS project. Pass `root` (the folder that contains app/), or create one with forge_new.",
            root.display()
        )
    })
}

fn routes_json(plan: &build::Plan) -> Value {
    json!({ "routes": plan.routes, "notFound": plan.not_found })
}

async fn call_tool(name: &str, args: &Value) -> Result<Value> {
    match name {
        "forge_docs_search" => {
            let query = arg_str(args, "query").context("`query` is required, e.g. {\"query\": \"loader redirect\"}")?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(3).clamp(1, 10) as usize;
            let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
            let ranked = search(query, arg_str(args, "doc"));
            if ranked.is_empty() {
                bail!(
                    "No section matches \"{query}\". Try fewer or different words, or read an outline with forge_docs_read."
                );
            }
            let total = ranked.len();
            let page: Vec<&(usize, Section)> = ranked.iter().skip(offset).take(limit).collect();
            let mut text = String::new();
            for (_, s) in &page {
                text.push_str(&format!("## {} › {}\n\n{}\n\n", s.doc, s.heading, s.body.trim()));
            }
            let more = total > offset + page.len();
            if more {
                text.push_str(&format!(
                    "_{} more matching sections: call again with offset {}._\n",
                    total - offset - page.len(),
                    offset + page.len()
                ));
            }
            let found: Vec<Value> = page.iter().map(|(_, s)| json!({ "doc": s.doc, "section": s.heading })).collect();
            Ok(text_result(text, Some(json!({ "total": total, "offset": offset, "hasMore": more, "sections": found }))))
        }
        "forge_docs_read" => {
            let doc =
                arg_str(args, "doc").context("`doc` is required: one of agents, guide, ui, design, architecture")?;
            let (name, summary, md) = DOCS.iter().find(|d| d.0 == doc).ok_or_else(|| {
                anyhow!("Unknown doc \"{doc}\". Available: {}", DOCS.iter().map(|d| d.0).collect::<Vec<_>>().join(", "))
            })?;
            let all = sections(name, md);
            match arg_str(args, "section") {
                None => {
                    let outline: Vec<&str> = all.iter().map(|s| s.heading.as_str()).collect();
                    let text = format!(
                        "# {name}\n\n{summary}\n\nSections (pass one as `section`):\n{}",
                        outline.iter().map(|h| format!("- {h}\n")).collect::<String>()
                    );
                    Ok(text_result(text, Some(json!({ "doc": name, "sections": outline }))))
                }
                Some(wanted) => {
                    let w = wanted.to_lowercase();
                    let found = all
                        .iter()
                        .find(|s| s.heading.to_lowercase() == w)
                        .or_else(|| all.iter().find(|s| s.heading.to_lowercase().contains(&w)))
                        .ok_or_else(|| anyhow!("No section \"{wanted}\" in {name}. Call forge_docs_read with only `doc` to see the outline."))?;
                    Ok(text_result(format!("## {}\n\n{}", found.heading, found.body.trim()), None))
                }
            }
        }
        "forge_ui_component" => match arg_str(args, "name") {
            None => {
                let names = ui_exports();
                let (components, functions): (Vec<&String>, Vec<&String>) =
                    names.iter().partition(|n| n.chars().next().is_some_and(char::is_uppercase));
                let join = |v: &[&String]| v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
                let text = format!(
                    "forge:ui exports\n\nComponents: {}\n\nHooks and functions: {}\n\nCall again with `name` for props. Usage: `import {{ Button }} from \"forge:ui\"`.",
                    join(&components),
                    join(&functions)
                );
                Ok(text_result(text, Some(json!({ "components": components, "functions": functions }))))
            }
            Some(name) => {
                let decl = ui_declaration(name).ok_or_else(|| {
                    let lower = name.to_lowercase();
                    let close: Vec<String> = ui_exports()
                        .into_iter()
                        .filter(|n| {
                            let candidate = n.to_lowercase();
                            candidate.contains(&lower)
                                || lower.contains(&candidate)
                                || edit_distance(&candidate, &lower) <= 2
                        })
                        .collect();
                    let hint = if close.is_empty() {
                        "Call without `name` to list all exports.".to_string()
                    } else {
                        format!("Did you mean: {}?", close.join(", "))
                    };
                    anyhow!("forge:ui has no export \"{name}\". {hint}")
                })?;
                Ok(text_result(format!("```ts\n// import {{ {name} }} from \"forge:ui\"\n{decl}\n```"), None))
            }
        },
        "forge_ui_themes" => {
            let query = arg_str(args, "query");
            let found = match query {
                Some(q) => find_themes(q),
                None => ui_themes(),
            };
            let mut text = String::new();
            if found.is_empty() {
                text.push_str("No preset matches those words. Build the look from Theme props instead (below), or omit `query` to list every preset.\n\n");
            }
            for theme in &found {
                text.push_str(&format!(
                    "## {} — `<Theme preset=\"{}\">`\n{}\nKeywords: {}\nSettings: `{}`\n\n",
                    theme["label"].as_str().unwrap_or_default(),
                    theme["name"].as_str().unwrap_or_default(),
                    theme["description"].as_str().unwrap_or_default(),
                    theme["keywords"]
                        .as_array()
                        .map(|k| k.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
                        .unwrap_or_default(),
                    theme["settings"]
                ));
            }
            text.push_str(
                "Customize any preset by passing props next to it; they win over the preset:\n\
                 - `accent`: ember, violet, cobalt, verdigris, gold, rose, steel, any CSS color (\"#0a7f5a\"), or a hue 0-360\n\
                 - `radius`: none, sm, md, lg, full    `density`: compact, comfortable, spacious\n\
                 - `surface`: solid or glass    `theme`: system, light, dark\n\
                 - `tokens`: { font, \"font-display\", surface, text, radius, … } for single values\n\
                 Example: `<Theme preset=\"liquid-glass\" accent=\"#ff5a36\" radius=\"full\">`",
            );
            Ok(text_result(text, Some(json!({ "themes": found }))))
        }
        "forge_templates" => {
            let list: Vec<Value> = scaffold::TEMPLATES
                .iter()
                .map(|(name, about, _)| json!({ "name": name, "description": about }))
                .collect();
            let text = scaffold::TEMPLATES.iter().map(|(n, a, _)| format!("- {n}: {a}\n")).collect::<String>();
            Ok(text_result(text, Some(json!({ "templates": list }))))
        }
        "forge_new" => {
            let dir = arg_str(args, "dir").context("`dir` is required, e.g. {\"dir\": \"my-app\"}")?;
            let template = arg_str(args, "template").unwrap_or("starter");
            scaffold::create(Path::new(dir), template)?;
            let files: Vec<String> = scaffold::TEMPLATES
                .iter()
                .find(|t| t.0 == template)
                .map(|t| t.2.iter().map(|f| f.0.to_string()).collect())
                .unwrap_or_default();
            let install = if template == "vanilla" { "" } else { "npm install, then " };
            let text = format!(
                "Created {dir} from the {template} template ({} files). Next: {install}`forge dev` in {dir}.",
                files.len()
            );
            Ok(text_result(text, Some(json!({ "dir": dir, "template": template, "files": files }))))
        }
        "forge_routes" => {
            let project = project(args, Target::Server, Mode::Prod)?;
            let plan = build::plan(&project)?;
            let value = routes_json(&plan);
            Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value)))
        }
        "forge_check" => {
            let project = project(args, Target::Server, Mode::Prod)?;
            let (plan, warnings) = build::check(&project).await?;
            let mut value = routes_json(&plan);
            value["ok"] = json!(true);
            value["warnings"] = json!(warnings);
            Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value)))
        }
        "forge_build" => {
            let target: Target = arg_str(args, "target").unwrap_or("server").parse().map_err(|e| anyhow!("{e}"))?;
            let project = project(args, target, Mode::Prod)?;
            let result = build::build(&project).await?;
            let out = if result.artifacts.is_empty() { project.out_dir() } else { project.dist_dir() };
            let value = json!({
                "ok": true,
                "target": target.as_str(),
                "output": out,
                "files": result.artifacts.iter().map(|(p, n)| json!({ "path": p, "bytes": n })).collect::<Vec<_>>(),
                "clientJsBytes": result.stats.client_bytes,
                "clientJsGzipBytes": result.stats.client_gzip,
                "cssBytes": result.stats.css_bytes,
                "routes": result.manifest.routes.len(),
                "warnings": result.warnings,
                "ms": result.elapsed.as_millis() as u64,
            });
            Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value)))
        }
        "forge_pack" => pack(args).await,
        other => bail!("Unknown tool \"{other}\". Call tools/list for the available tools."),
    }
}

#[cfg(feature = "pack")]
async fn pack(args: &Value) -> Result<Value> {
    let input =
        arg_str(args, "input").context("`input` is required: the path of an .html, .jsx, .tsx, .js or .ts file")?;
    let output = arg_str(args, "output").map(PathBuf::from);
    let summary = crate::pack::run(Path::new(input), output.as_deref(), crate::pack::Options::default()).await?;
    let value = json!({
        "ok": true,
        "output": summary.output,
        "bytes": summary.bytes,
        "packages": summary.packages,
        "notes": summary.notes,
        "warnings": summary.warnings,
    });
    Ok(text_result(serde_json::to_string_pretty(&value)?, Some(value)))
}

#[cfg(not(feature = "pack"))]
async fn pack(_: &Value) -> Result<Value> {
    bail!("This forge binary was built without the `pack` feature.")
}

/// Levenshtein distance, for "did you mean" suggestions.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != cb)).min(row[j] + 1).min(current + 1);
            previous = current;
        }
    }
    row[b.len()]
}

// ---- prompts ----

const PROMPTS: &[(&str, &str, &str)] = &[
    (
        "forge_build_app",
        "Plan and build an application with Next.RS and forge:ui",
        "Build this with Next.RS: {idea}\n\nWork in this order:\n1. Read the `agents` doc (forge_docs_read) and the design rules (`design`).\n2. Create the project with forge_new (template `starter` unless another fits better) and run `npm install`.\n3. Look up each forge:ui component you use with forge_ui_component, and each framework feature with forge_docs_search.\n4. Put data in loader.ts files, mutations in app/api routes, and UI in pages built from forge:ui components.\n5. After each change run forge_check, then `npx tsc -p .`. Fix every error before continuing.\n6. Finish with forge_build for the target the user wants and go through the checklist at the end of the design rules.",
    ),
    (
        "forge_build_game",
        "Plan and build a game with Next.RS",
        "Build this game with Next.RS: {idea}\n\nWork in this order:\n1. Read the `agents` doc, then search the guide for \"forge:game\" and \"single-html\".\n2. Start from the `three` template for 3D or `vanilla` for 2D canvas (forge_new). Make the page client-only (`export const ssr = false`) if it touches WebGL at module scope.\n3. Run the simulation in a fixed-step `loop()` from forge:game, read input with `input()`, size the canvas with `fit()`.\n4. Put UI over the canvas in `Hud` slots from forge:ui; menus are `Dialog`s. Save progress with forge:store.\n5. Check with forge_check and build with forge_build target `single-html`: the result is one file that runs offline.",
    ),
];

// ---- JSON-RPC ----

fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

/// Answer one JSON-RPC message. Notifications (no `id`) get no reply.
pub async fn handle(message: &Value) -> Option<Value> {
    let method = message.get("method").and_then(Value::as_str)?;
    let id = message.get("id").cloned()?;
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
            ok(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                    "serverInfo": { "name": "forge", "title": "Next.RS (Forge)", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Tools for building apps and games with Next.RS. Read the `agents` doc first (forge_docs_read), search the docs before using a feature (forge_docs_search), look up forge:ui components (forge_ui_component), and run forge_check after every change."
                }),
            )
        }
        "ping" => ok(&id, json!({})),
        "tools/list" => ok(&id, json!({ "tools": tools() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(err(&id, -32602, "tools/call needs params.name"));
            };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(name, &args).await {
                Ok(result) => ok(&id, result),
                // Tool failures are results the model can read and act on, not protocol errors.
                Err(e) => {
                    ok(&id, json!({ "content": [{ "type": "text", "text": format!("{e:#}") }], "isError": true }))
                }
            }
        }
        "resources/list" => {
            let mut list: Vec<Value> = DOCS
                .iter()
                .map(|(name, about, _)| json!({ "uri": format!("forge://docs/{name}"), "name": name, "description": about, "mimeType": "text/markdown" }))
                .collect();
            list.push(json!({ "uri": "forge://ui/types", "name": "forge-ui.d.ts", "description": "TypeScript declarations of forge:ui", "mimeType": "text/plain" }));
            ok(&id, json!({ "resources": list }))
        }
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or_default();
            let found = match uri {
                "forge://ui/types" => Some(("text/plain", UI_DTS)),
                _ => uri
                    .strip_prefix("forge://docs/")
                    .and_then(|n| DOCS.iter().find(|d| d.0 == n))
                    .map(|d| ("text/markdown", d.2)),
            };
            match found {
                Some((mime, text)) => ok(&id, json!({ "contents": [{ "uri": uri, "mimeType": mime, "text": text }] })),
                None => err(&id, -32002, format!("Unknown resource {uri}")),
            }
        }
        "prompts/list" => {
            let list: Vec<Value> = PROMPTS
                .iter()
                .map(|(name, about, _)| json!({ "name": name, "description": about, "arguments": [{ "name": "idea", "description": "What to build", "required": true }] }))
                .collect();
            ok(&id, json!({ "prompts": list }))
        }
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let idea = params.pointer("/arguments/idea").and_then(Value::as_str).unwrap_or("(describe the project)");
            match PROMPTS.iter().find(|p| p.0 == name) {
                Some((_, about, template)) => ok(
                    &id,
                    json!({
                        "description": about,
                        "messages": [{ "role": "user", "content": { "type": "text", "text": template.replace("{idea}", idea) } }]
                    }),
                ),
                None => err(&id, -32602, format!("Unknown prompt {name}")),
            }
        }
        _ => err(&id, -32601, format!("Method not found: {method}")),
    })
}

/// Serve MCP on stdin/stdout until stdin closes. Nothing but protocol messages goes to stdout.
pub async fn serve_stdio() -> Result<()> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || -> Result<()> {
        use std::io::{BufRead, Write};
        let stdin = std::io::stdin();
        let mut stdout = std::io::stdout();
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = match serde_json::from_str::<Value>(&line) {
                Ok(message) => handle.block_on(self::handle(&message)),
                Err(e) => Some(err(&Value::Null, -32700, format!("Parse error: {e}"))),
            };
            if let Some(reply) = reply {
                writeln!(stdout, "{reply}")?;
                stdout.flush()?;
            }
        }
        Ok(())
    })
    .await?
}

/// Serve MCP over Streamable HTTP (stateless JSON): `POST /mcp` with one JSON-RPC message,
/// or a batch. Browsers are refused unless they come from localhost (DNS rebinding).
pub async fn serve_http(host: &str, port: u16) -> Result<()> {
    use axum::{
        Router,
        body::Bytes,
        http::{HeaderMap, StatusCode, header},
        response::IntoResponse,
        routing::post,
    };

    async fn endpoint(headers: HeaderMap, body: Bytes) -> axum::response::Response {
        let local = |v: &str| ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|p| v.starts_with(p));
        if headers.get("origin").and_then(|v| v.to_str().ok()).is_some_and(|o| !local(o)) {
            return (StatusCode::FORBIDDEN, "cross-origin requests are not allowed").into_response();
        }
        let reply = match serde_json::from_slice::<Value>(&body) {
            Err(e) => Some(err(&Value::Null, -32700, format!("Parse error: {e}"))),
            Ok(Value::Array(batch)) => {
                let mut replies = Vec::new();
                for message in &batch {
                    replies.extend(handle(message).await);
                }
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(single) => handle(&single).await,
        };
        match reply {
            Some(reply) => ([(header::CONTENT_TYPE, "application/json")], reply.to_string()).into_response(),
            // Notifications and responses are acknowledged without a body.
            None => StatusCode::ACCEPTED.into_response(),
        }
    }

    let app = Router::new().route("/mcp", post(endpoint).get(|| async { StatusCode::METHOD_NOT_ALLOWED }));
    let listener =
        tokio::net::TcpListener::bind((host, port)).await.with_context(|| format!("binding {host}:{port}"))?;
    eprintln!("forge mcp → http://{}/mcp", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_follow_headings_and_skip_code_fences() {
        let md = "intro\n\n## One\na\n```\n## not a heading\n```\n### Deep\nb\n## Two\nc\n";
        let s = sections("t", md);
        let headings: Vec<&str> = s.iter().map(|x| x.heading.as_str()).collect();
        assert_eq!(headings, ["Overview", "One", "One › Deep", "Two"]);
        assert!(s[1].body.contains("## not a heading"));
    }

    #[test]
    fn search_prefers_headings_and_full_matches() {
        let top = search("loaders redirect", Some("guide"));
        assert!(
            top[0].1.heading.contains("Loaders"),
            "{:?}",
            top.iter().take(3).map(|t| &t.1.heading).collect::<Vec<_>>()
        );
        assert!(search("zzzznotaword", None).is_empty());
        let ui = search("Table sortable", None);
        assert_eq!(ui[0].1.doc, "ui");
    }

    #[test]
    fn theme_catalog_is_readable_and_searchable() {
        let themes = ui_themes();
        assert!(themes.len() >= 8, "the JSON block in themes.ts must parse: {} themes", themes.len());
        for theme in &themes {
            assert!(
                theme["name"].is_string() && theme["description"].is_string() && theme["settings"].is_object(),
                "{theme}"
            );
            assert!(theme["keywords"].as_array().is_some_and(|k| k.len() >= 5), "{theme}");
        }
        let name = |q: &str| find_themes(q).first().map(|t| t["name"].as_str().unwrap().to_string());
        assert_eq!(name("frosted apple glass").as_deref(), Some("liquid-glass"));
        assert_eq!(name("dense admin panel").as_deref(), Some("graphite"));
        assert_eq!(name("hacker console").as_deref(), Some("terminal"));
        assert_eq!(name("a blog with serif type").as_deref(), Some("paper"));
        assert_eq!(name("game menu").as_deref(), Some("arcade"));
        assert!(find_themes("xyzzy").is_empty());
    }

    #[test]
    fn ui_lookup_returns_declarations() {
        let names = ui_exports();
        for expected in ["Button", "Theme", "Table", "CommandPalette", "useHotkey", "toast"] {
            assert!(names.iter().any(|n| n == expected), "{expected} missing from {names:?}");
        }
        let button = ui_declaration("Button").unwrap();
        assert!(
            button.contains("export function Button(") && button.contains("export interface ButtonProps"),
            "{button}"
        );
        assert!(button.contains("loading?: boolean"), "{button}");
        let table = ui_declaration("Table").unwrap();
        assert!(table.contains("export function Table<Row>") && table.contains("caption: string"), "{table}");
        assert!(ui_declaration("Nope").is_none());
    }
}
