//! Static analysis of app source with the Oxc parser: one parse yields the
//! module's export names, literal config exports, and security-policy violations.

use std::{collections::BTreeMap, fmt, path::Path};

use anyhow::{Result, bail};
use oxc::{
    allocator::Allocator,
    ast::ast::*,
    ast_visit::{Visit, walk},
    parser::Parser,
    span::{SourceType, Span},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Eval,
    NewFunction,
    InternalImport,
    RemoteImport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub rule: Rule,
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}: {}", self.file, self.line, self.column, self.message)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Policy {
    pub allow_eval: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(f64),
    Bool(bool),
}

#[derive(Debug, Default)]
pub struct Analysis {
    /// Export names; the default export is `"default"`.
    pub exports: Vec<String>,
    /// `export const name = <number|boolean literal>`.
    pub literals: BTreeMap<String, Literal>,
    pub violations: Vec<Violation>,
}

impl Analysis {
    pub fn has(&self, name: &str) -> bool {
        self.exports.iter().any(|e| e == name)
    }
    pub fn number(&self, name: &str) -> Option<f64> {
        match self.literals.get(name) {
            Some(Literal::Number(n)) => Some(*n),
            _ => None,
        }
    }
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.literals.get(name), Some(Literal::Bool(true)))
    }
}

/// Specifiers app code may never import: engine internals and generated files.
pub fn is_internal_specifier(spec: &str) -> bool {
    spec.starts_with("ext:") || spec.starts_with("forge:internal") || spec.contains(".forge/generated")
}

/// 1-based line and column (in characters) of a byte offset.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut end = offset.min(source.len());
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let before = &source[..end];
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = before.rfind('\n').map_or(before, |i| &before[i + 1..]).chars().count() + 1;
    (line, column)
}

pub fn analyze(file: &str, source: &str, policy: Policy) -> Result<Analysis> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(Path::new(file)).unwrap_or_else(|_| SourceType::tsx());
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.fatal_error || parsed.diagnostics.has_errors() {
        let Some(err) = parsed.diagnostics.errors().next() else { bail!("{file}: syntax error") };
        let at = err.labels.first().map(|l| line_col(source, l.offset() as usize));
        match at {
            Some((line, col)) => bail!("{file}:{line}:{col}: {}", err.message),
            None => bail!("{file}: {}", err.message),
        }
    }

    let mut out = Analysis::default();
    for stmt in &parsed.program.body {
        match stmt {
            Statement::ExportDefaultDeclaration(_) => out.exports.push("default".into()),
            Statement::ExportNamedDeclaration(decl) if !decl.export_kind.is_type() => {
                out.exports.extend(
                    decl.specifiers.iter().filter(|s| !s.export_kind.is_type()).map(|s| s.exported.name().to_string()),
                );
            }
            Statement::ExportFromDeclaration(decl) if !decl.export_kind.is_type() => {
                out.exports.extend(
                    decl.specifiers.iter().filter(|s| !s.export_kind.is_type()).map(|s| s.exported.name().to_string()),
                );
            }
            Statement::ExportDeclaration(decl) => match &decl.declaration {
                Declaration::FunctionDeclaration(f) => out.exports.extend(f.id.as_ref().map(|id| id.name.to_string())),
                Declaration::ClassDeclaration(c) => out.exports.extend(c.id.as_ref().map(|id| id.name.to_string())),
                Declaration::VariableDeclaration(v) => {
                    for d in &v.declarations {
                        out.exports.extend(d.id.get_binding_identifiers().iter().map(|id| id.name.to_string()));
                        if let (Some(id), Some(init)) = (d.id.get_binding_identifier(), &d.init) {
                            let lit = match init.without_parentheses() {
                                Expression::NumericLiteral(n) => Some(Literal::Number(n.value)),
                                Expression::BooleanLiteral(b) => Some(Literal::Bool(b.value)),
                                _ => None,
                            };
                            out.literals.extend(lit.map(|l| (id.name.to_string(), l)));
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    let mut scan = Scan { file, source, policy, out: &mut out.violations };
    scan.visit_program(&parsed.program);
    Ok(out)
}

struct Scan<'s> {
    file: &'s str,
    source: &'s str,
    policy: Policy,
    out: &'s mut Vec<Violation>,
}

impl Scan<'_> {
    fn report(&mut self, span: Span, rule: Rule, message: String) {
        let (line, column) = line_col(self.source, span.start as usize);
        self.out.push(Violation { file: self.file.to_string(), line, column, rule, message });
    }

    fn check_specifier(&mut self, span: Span, spec: Option<&str>) {
        let Some(spec) = spec else { return };
        if is_internal_specifier(spec) {
            self.report(
                span,
                Rule::InternalImport,
                format!("`{spec}` is internal to Forge and cannot be imported from app code"),
            );
        } else if ["http:", "https:", "data:", "blob:"].iter().any(|p| spec.starts_with(p)) {
            self.report(
                span,
                Rule::RemoteImport,
                format!("remote import `{spec}` is denied: dependencies must be bundled"),
            );
        }
    }

    fn check_function_ctor(&mut self, callee: &Expression<'_>, span: Span) {
        if self.policy.allow_eval {
            return;
        }
        if let Expression::Identifier(id) = callee.without_parentheses() {
            if id.name == "Function" {
                self.report(
                    span,
                    Rule::NewFunction,
                    "`new Function` compiles code from strings and is denied (security.allowEval)".into(),
                );
            }
        }
    }
}

impl<'a> Visit<'a> for Scan<'_> {
    // Any reference to `eval` is the global: ES modules cannot bind that name.
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if !self.policy.allow_eval && it.name == "eval" {
            self.report(it.span, Rule::Eval, "`eval` is denied (security.allowEval)".into());
        }
    }

    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if !self.policy.allow_eval && it.property.name == "eval" {
            if let Expression::Identifier(obj) = it.object.without_parentheses() {
                if matches!(obj.name.as_str(), "globalThis" | "window" | "self") {
                    self.report(it.span, Rule::Eval, "`eval` is denied (security.allowEval)".into());
                }
            }
        }
        walk::walk_static_member_expression(self, it);
    }

    fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
        self.check_function_ctor(&it.callee, it.span);
        walk::walk_new_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        self.check_function_ctor(&it.callee, it.span);
        walk::walk_call_expression(self, it);
    }

    fn visit_import_declaration(&mut self, it: &ImportDeclaration<'a>) {
        self.check_specifier(it.source.span, it.source.value.as_str());
    }

    fn visit_export_from_declaration(&mut self, it: &ExportFromDeclaration<'a>) {
        self.check_specifier(it.source.span, it.source.value.as_str());
    }

    fn visit_export_all_declaration(&mut self, it: &ExportAllDeclaration<'a>) {
        self.check_specifier(it.source.span, it.source.value.as_str());
    }

    fn visit_import_expression(&mut self, it: &ImportExpression<'a>) {
        if let Expression::StringLiteral(s) = it.source.without_parentheses() {
            self.check_specifier(s.span, s.value.as_str());
        }
        walk::walk_import_expression(self, it);
    }
}

/// A `new URL("./relative", import.meta.url)` expression: byte range and specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetUrl {
    pub start: usize,
    pub end: usize,
    pub specifier: String,
}

/// Find every `new URL("./x" | "../x", import.meta.url)` in a module (workers, wasm, models…).
pub fn find_asset_urls(file: &str, source: &str) -> Result<Vec<AssetUrl>> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(Path::new(file)).unwrap_or_else(|_| SourceType::tsx());
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.fatal_error || parsed.diagnostics.has_errors() {
        bail!("{file}: syntax error");
    }
    struct Finder(Vec<AssetUrl>);
    impl<'a> Visit<'a> for Finder {
        fn visit_new_expression(&mut self, it: &NewExpression<'a>) {
            let is_url = matches!(it.callee.without_parentheses(), Expression::Identifier(id) if id.name == "URL");
            if is_url && it.arguments.len() == 2 {
                let spec = match it.arguments[0].as_expression() {
                    Some(Expression::StringLiteral(s)) => s.value.as_str().map(str::to_string),
                    _ => None,
                };
                let base_is_meta_url = matches!(
                    it.arguments[1].as_expression(),
                    Some(Expression::StaticMemberExpression(m))
                        if m.property.name == "url" && matches!(m.object.without_parentheses(), Expression::ImportMeta(_))
                );
                if let (Some(spec), true) = (spec, base_is_meta_url) {
                    if spec.starts_with("./") || spec.starts_with("../") {
                        self.0.push(AssetUrl {
                            start: it.span.start as usize,
                            end: it.span.end as usize,
                            specifier: spec,
                        });
                        return;
                    }
                }
            }
            walk::walk_new_expression(self, it);
        }
    }
    let mut finder = Finder(Vec::new());
    finder.visit_program(&parsed.program);
    Ok(finder.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(src: &str) -> Vec<Rule> {
        analyze("app/page.tsx", src, Policy::default()).unwrap().violations.into_iter().map(|v| v.rule).collect()
    }

    #[test]
    fn denies_eval() {
        assert_eq!(rules("const x = eval('1+1');"), [Rule::Eval]);
        assert_eq!(rules("const x = (0, eval)('1');"), [Rule::Eval]);
        assert_eq!(rules("globalThis.eval('1');"), [Rule::Eval]);
    }

    #[test]
    fn denies_new_function() {
        assert_eq!(rules("const f = new Function('a', 'return a');"), [Rule::NewFunction]);
        assert_eq!(rules("const f = Function('return 1');"), [Rule::NewFunction]);
    }

    #[test]
    fn denies_internal_imports() {
        assert_eq!(rules("import x from 'ext:forge_runtime/ops.js';"), [Rule::InternalImport]);
        assert_eq!(rules("export * from 'ext:forge_cache';"), [Rule::InternalImport]);
        assert_eq!(rules("const m = await import('forge:internal/server');"), [Rule::InternalImport]);
        assert_eq!(rules("import x from '../.forge/generated/runtime/server.js';"), [Rule::InternalImport]);
    }

    #[test]
    fn denies_remote_imports() {
        assert_eq!(rules("import x from 'https://cdn.example.com/x.js';"), [Rule::RemoteImport]);
        assert_eq!(rules("const m = await import('data:text/javascript,1');"), [Rule::RemoteImport]);
    }

    #[test]
    fn ignores_strings_comments_and_lookalikes() {
        let src = r#"
            // eval("nope")
            const a = "eval('x') new Function('y') ext:forge";
            const b = `import 'ext:forge_x'`;
            const evaluate = (x: number) => x; evaluate(1);
            const ok = obj.eval; const isFn = f instanceof Function;
            import { useData } from "forge:data";
        "#;
        assert_eq!(rules(src), []);
    }

    #[test]
    fn policy_can_allow_eval_but_never_internal_imports() {
        let a = analyze("a.js", "eval('1'); new Function('x'); import 'ext:x';", Policy { allow_eval: true }).unwrap();
        assert_eq!(a.violations.iter().map(|v| v.rule).collect::<Vec<_>>(), [Rule::InternalImport]);
    }

    #[test]
    fn finds_asset_urls() {
        let src = "const w = new Worker(new URL('./sim.worker.ts', import.meta.url), { type: 'module' });\n\
                   const m = new URL(\"../models/net.onnx\", import.meta.url);\n\
                   const skip1 = new URL('https://x', import.meta.url);\n\
                   const skip2 = new URL('./a.js', location.href);\n\
                   const skip3 = new URL(name, import.meta.url);";
        let found = find_asset_urls("app/x.ts", src).unwrap();
        let specs: Vec<_> = found.iter().map(|f| f.specifier.as_str()).collect();
        assert_eq!(specs, ["./sim.worker.ts", "../models/net.onnx"]);
        assert_eq!(&src[found[0].start..found[0].end], "new URL('./sim.worker.ts', import.meta.url)");
    }

    #[test]
    fn reports_position() {
        let v = &analyze("app/x.js", "const a = 1;\n  eval('x');", Policy::default()).unwrap().violations[0];
        assert_eq!((v.line, v.column), (2, 3));
        assert!(v.to_string().starts_with("app/x.js:2:3: "));
    }

    #[test]
    fn collects_exports_and_literals() {
        let a = analyze(
            "app/page.tsx",
            r#"
            export default function Page() { return <main/>; }
            export const revalidate = 60;
            export const streaming = true;
            export const meta = { title: "x" };
            export function mount(el: HTMLElement) {}
            export type Props = { a: number };
            const hidden = 1; export { hidden as shown };
            "#,
            Policy::default(),
        )
        .unwrap();
        assert_eq!(a.exports, ["default", "revalidate", "streaming", "meta", "mount", "shown"]);
        assert_eq!(a.number("revalidate"), Some(60.0));
        assert!(a.flag("streaming"));
        assert!(a.has("mount") && !a.has("Props"));
    }

    #[test]
    fn syntax_errors_are_reported_with_position() {
        let e =
            analyze("app/page.jsx", "const ok = 1;\nexport default function Home( {", Policy::default()).unwrap_err();
        assert!(e.to_string().starts_with("app/page.jsx:2:"), "{e}");
    }
}
