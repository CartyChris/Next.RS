//! `forge pack App.tsx`: one component or script file → one HTML app.
//!
//! A default-exported React component is mounted with `react-dom/client` (Preact with
//! `preact` when the file imports it); a module that exports `mount(el)` gets the root
//! element; anything else simply runs. Imports install from npm, CSS imports are inlined,
//! and Tailwind's browser build is added when the markup uses Tailwind classes and the file
//! brings no stylesheet of its own. The generated page then goes through the HTML packer.

use std::path::Path;

use anyhow::{Result, bail};
use oxc::{
    allocator::Allocator,
    ast::ast::*,
    ast_visit::{Visit, walk},
    parser::Parser,
    span::SourceType,
};

use crate::build::html::escape_html;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Shape {
    pub default_export: bool,
    pub jsx: bool,
    pub react: bool,
    pub preact: bool,
    pub mount: bool,
    pub css: bool,
    /// Distinct class names that look like Tailwind utilities.
    pub tailwind_classes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Kind {
    React,
    Preact,
    Mount,
    Script,
}

impl Shape {
    pub fn kind(&self) -> Kind {
        match (self.default_export, self.preact, self.jsx || self.react, self.mount) {
            (true, true, _, _) => Kind::Preact,
            (true, false, true, _) => Kind::React,
            (_, _, _, true) => Kind::Mount,
            _ => Kind::Script,
        }
    }

    /// Tailwind classes in the markup and no stylesheet of its own: an artifact-style component.
    pub fn wants_tailwind(&self) -> bool {
        !self.css && self.tailwind_classes >= 6
    }
}

const TAILWIND_PREFIXES: &[&str] = &[
    "p-",
    "px-",
    "py-",
    "pt-",
    "pb-",
    "pl-",
    "pr-",
    "m-",
    "mx-",
    "my-",
    "mt-",
    "mb-",
    "ml-",
    "mr-",
    "w-",
    "h-",
    "min-w-",
    "min-h-",
    "max-w-",
    "max-h-",
    "text-",
    "bg-",
    "border-",
    "rounded",
    "shadow",
    "gap-",
    "items-",
    "justify-",
    "font-",
    "leading-",
    "tracking-",
    "space-x-",
    "space-y-",
    "ring-",
    "opacity-",
    "duration-",
    "grid-cols-",
    "col-span-",
    "z-",
    "inset-",
    "top-",
    "left-",
    "right-",
    "bottom-",
    "from-",
    "to-",
    "via-",
    "hover:",
    "focus:",
    "sm:",
    "md:",
    "lg:",
    "xl:",
    "dark:",
    "aspect-",
    "overflow-",
    "cursor-",
    "select-",
    "translate-",
    "scale-",
    "rotate-",
    "animate-",
    "line-clamp-",
];
const TAILWIND_WORDS: &[&str] = &[
    "flex",
    "grid",
    "hidden",
    "block",
    "inline-flex",
    "inline-block",
    "relative",
    "absolute",
    "fixed",
    "sticky",
    "container",
    "truncate",
    "uppercase",
    "italic",
    "underline",
    "transition",
    "flex-col",
    "flex-row",
    "flex-1",
    "flex-wrap",
    "mx-auto",
    "rounded",
    "shadow",
    "border",
    "outline-none",
];

fn tailwind_like(class: &str) -> bool {
    TAILWIND_WORDS.contains(&class) || TAILWIND_PREFIXES.iter().any(|p| class.starts_with(p) && class.len() > p.len())
}

#[derive(Default)]
struct Finder {
    shape: Shape,
    classes: std::collections::BTreeSet<String>,
}

impl Finder {
    fn class_list(&mut self, s: &str) {
        for c in s.split_whitespace() {
            if tailwind_like(c) {
                self.classes.insert(c.to_string());
            }
        }
    }
}

impl<'a> Visit<'a> for Finder {
    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        let source = it.source.value.as_str().unwrap_or_default();
        let name = source.split('/').next().unwrap_or("");
        self.shape.react |= matches!(name, "react" | "react-dom");
        self.shape.preact |= name == "preact" || source.starts_with("@preact/");
        self.shape.css |= source.ends_with(".css");
    }
    fn visit_export_default_declaration(&mut self, it: &ExportDefaultDeclaration<'a>) {
        self.shape.default_export = true;
        walk::walk_export_default_declaration(self, it);
    }
    fn visit_export_named_declaration(&mut self, it: &ExportNamedDeclaration<'a>) {
        for s in &it.specifiers {
            let exported = s.exported.name();
            self.shape.default_export |= exported == "default";
            self.shape.mount |= exported == "mount";
        }
    }
    fn visit_export_declaration(&mut self, it: &ExportDeclaration<'a>) {
        if let Declaration::FunctionDeclaration(f) = &it.declaration {
            self.shape.mount |= f.id.as_ref().is_some_and(|id| id.name == "mount");
        }
        if let Declaration::VariableDeclaration(v) = &it.declaration {
            self.shape.mount |= v.declarations.iter().any(|d| d.id.get_identifier_name().is_some_and(|n| n == "mount"));
        }
        walk::walk_export_declaration(self, it);
    }
    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        self.shape.jsx = true;
        walk::walk_jsx_element(self, it);
    }
    fn visit_jsx_fragment(&mut self, it: &JSXFragment<'a>) {
        self.shape.jsx = true;
        walk::walk_jsx_fragment(self, it);
    }
    fn visit_jsx_attribute(&mut self, it: &JSXAttribute<'a>) {
        let is_class =
            matches!(&it.name, JSXAttributeName::Identifier(id) if id.name == "className" || id.name == "class");
        if is_class {
            match &it.value {
                Some(JSXAttributeValue::StringLiteral(s)) => self.class_list(s.value.as_str().unwrap_or_default()),
                Some(JSXAttributeValue::ExpressionContainer(c)) => {
                    // `className={`a ${b ? "c" : "d"}`}` and `cn("a", cond && "b")`
                    let mut strings = Strings::default();
                    strings.visit_jsx_expression_container(c);
                    for s in strings.0 {
                        self.class_list(&s);
                    }
                }
                _ => {}
            }
        }
        walk::walk_jsx_attribute(self, it);
    }
}

#[derive(Default)]
struct Strings(Vec<String>);

impl<'a> Visit<'a> for Strings {
    fn visit_string_literal(&mut self, it: &StringLiteral<'a>) {
        self.0.push(it.value.as_str().unwrap_or_default().to_string());
    }
    fn visit_template_element(&mut self, it: &TemplateElement<'a>) {
        if let Some(c) = it.value.cooked.and_then(|c| c.as_str()) {
            self.0.push(c.to_string());
        }
    }
}

pub fn analyze(code: &str, path: &Path) -> Result<Shape> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::tsx()).with_module(true);
    let parsed = Parser::new(&allocator, code, source_type).parse();
    if let Some(e) = parsed.diagnostics.errors().next() {
        bail!("{}: {e}", path.display());
    }
    let mut finder = Finder::default();
    finder.visit_program(&parsed.program);
    finder.shape.tailwind_classes = finder.classes.len();
    Ok(finder.shape)
}

/// The page that mounts `file` (a name next to the page), for the HTML packer.
pub fn page(file: &str, title: &str, kind: &Kind, tailwind: bool) -> String {
    let spec = serde_json::to_string(&format!("./{file}")).expect("string");
    let script = match kind {
        Kind::React => format!(
            "import App from {spec};\nimport {{ createElement }} from \"react\";\nimport {{ createRoot }} from \"react-dom/client\";\ncreateRoot(document.getElementById(\"root\")).render(createElement(App));"
        ),
        Kind::Preact => format!(
            "import App from {spec};\nimport {{ h, render }} from \"preact\";\nrender(h(App, null), document.getElementById(\"root\"));"
        ),
        Kind::Mount => format!("import * as app from {spec};\napp.mount(document.getElementById(\"root\"));"),
        Kind::Script => format!("import {spec};"),
    };
    let tailwind =
        if tailwind { "<script src=\"https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4\"></script>" } else { "" };
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title>{tailwind}</head><body><div id=\"root\"></div><script type=\"module\">\n{script}\n</script></body></html>",
        escape_html(title)
    )
}

/// `counter-app.tsx` → "Counter App".
pub fn title_from(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    let mut prev_lower = false;
    for c in stem.chars() {
        if c == '-' || c == '_' || c == '.' || c == ' ' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        if out.is_empty() || out.ends_with(' ') {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() { "App".into() } else { t }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(src: &str, file: &str) -> Shape {
        analyze(src, Path::new(file)).unwrap()
    }

    #[test]
    fn recognizes_what_a_file_is() {
        let react = shape(
            r#"import { useState } from "react";
export default function App() { const [n, set] = useState(0); return <div className="flex items-center gap-2 p-4 rounded-lg bg-slate-900 text-white"><button className={`px-3 py-1 ${n ? "font-bold" : ""}`}>{n}</button></div>; }"#,
            "App.tsx",
        );
        assert_eq!(react.kind(), Kind::React);
        assert!(react.wants_tailwind(), "{react:?}");
        let styled = shape(
            "import './app.css';\nexport default () => <div className=\"flex items-center gap-2 p-4 rounded bg-x text-y\" />;",
            "a.jsx",
        );
        assert!(!styled.wants_tailwind());
        assert_eq!(
            shape("import { h } from 'preact';\nexport default function A() { return <p/>; }", "a.jsx").kind(),
            Kind::Preact
        );
        assert_eq!(shape("export function mount(el) { el.textContent = 'hi'; }", "a.js").kind(), Kind::Mount);
        assert_eq!(shape("const mount = (el) => {};\nexport { mount };", "a.js").kind(), Kind::Mount);
        assert_eq!(shape("document.body.append('hi');", "a.js").kind(), Kind::Script);
        // A default export without JSX or React is not a component.
        assert_eq!(shape("export default 42;", "a.js").kind(), Kind::Script);
        assert!(analyze("export default () => <div", Path::new("a.jsx")).is_err());
    }

    #[test]
    fn generated_pages() {
        let p = page("App.tsx", "My <App>", &Kind::React, true);
        assert!(
            p.contains(r#"import App from "./App.tsx";"#)
                && p.contains("createRoot(document.getElementById(\"root\"))")
        );
        assert!(p.contains("<title>My &lt;App&gt;</title>") && p.contains("@tailwindcss/browser@4"));
        assert!(!page("a.js", "A", &Kind::Script, false).contains("tailwind"));
        assert_eq!(title_from(Path::new("counter-app.tsx")), "Counter App");
        assert_eq!(title_from(Path::new("TodoList.jsx")), "Todo List");
        assert_eq!(title_from(Path::new("game_of_life2.js")), "Game Of Life2");
    }
}
