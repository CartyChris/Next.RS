//! CSS for packed pages: `@import`ed sheets are inlined and `url()` references (fonts,
//! images) become data URLs, all resolved against the stylesheet's own URL.

use anyhow::Result;
use url::Url;

use super::Packer;

/// A reference found in CSS: byte range to replace and what kind it is.
#[derive(Debug, PartialEq, Eq)]
enum Ref {
    /// `url(...)` at `start..end` (the whole function).
    Url { start: usize, end: usize, target: String },
    /// `@import ... ;` at `start..end`, with any media / layer condition.
    Import { start: usize, end: usize, target: String, condition: String },
}

/// Find `url()` and `@import` outside comments and strings.
fn scan(css: &str) -> Vec<Ref> {
    let b = css.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let skip_string = |mut j: usize, q: u8| {
        j += 1;
        while j < b.len() && b[j] != q {
            j += if b[j] == b'\\' { 2 } else { 1 };
        }
        (j + 1).min(b.len())
    };
    let skip_ws = |mut j: usize| {
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        j
    };
    // `url(...)` or a string starting at `j`: (target, end).
    let target_at = |j: usize| -> Option<(String, usize)> {
        if b.get(j).is_some_and(|c| *c == b'"' || *c == b'\'') {
            let end = skip_string(j, b[j]);
            return Some((unescape(&css[j + 1..end.saturating_sub(1).max(j + 1)]), end));
        }
        if css.len() >= j + 4 && css[j..j + 4].eq_ignore_ascii_case("url(") {
            let k = skip_ws(j + 4);
            let (target, close) = if b.get(k).is_some_and(|c| *c == b'"' || *c == b'\'') {
                let e = skip_string(k, b[k]);
                (unescape(&css[k + 1..e.saturating_sub(1).max(k + 1)]), skip_ws(e))
            } else {
                let e = css[k..].find(')').map_or(b.len(), |x| k + x);
                (css[k..e].trim().to_string(), e)
            };
            if b.get(close) == Some(&b')') {
                return Some((target, close + 1));
            }
        }
        None
    };
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = css[i + 2..].find("*/").map_or(b.len(), |x| i + 2 + x + 2);
            }
            b'"' | b'\'' => i = skip_string(i, b[i]),
            b'@' if css.len() >= i + 7 && css[i + 1..i + 7].eq_ignore_ascii_case("import") => {
                let j = skip_ws(i + 7);
                match target_at(j) {
                    Some((target, after)) => {
                        let end = css[after..].find(';').map_or(b.len(), |x| after + x + 1);
                        let condition = css[after..end].trim_end_matches(';').trim().to_string();
                        out.push(Ref::Import { start: i, end, target, condition });
                        i = end;
                    }
                    None => i += 1,
                }
            }
            b'u' | b'U' if i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'-' || b[i - 1] == b'_') => {
                match target_at(i) {
                    Some((target, end)) => {
                        out.push(Ref::Url { start: i, end, target });
                        i = end;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// CSS string escapes: `\"`, `\\`, and hex escapes like `\26 `.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
            hex.push(chars.next().unwrap());
        }
        if hex.is_empty() {
            if let Some(n) = chars.next().filter(|n| *n != '\n') {
                out.push(n);
            }
        } else {
            if chars.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                chars.next();
            }
            out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('\u{FFFD}'));
        }
    }
    out
}

/// References that stay as they are: fragments, data/blob URLs, empty.
fn keep(target: &str) -> bool {
    let t = target.trim();
    t.is_empty() || t.starts_with('#') || t.starts_with("data:") || t.starts_with("blob:") || t.starts_with("about:")
}

impl Packer {
    /// `css` (fetched from `base`) with imports inlined and assets embedded. Unloadable
    /// references stay as written, with a warning.
    pub async fn inline_css(&self, css: &str, base: &Url, depth: u8) -> Result<String> {
        let refs = scan(css);
        let mut parts: Vec<Option<String>> = Vec::with_capacity(refs.len());
        let loads = refs.iter().map(|r| async move {
            let (Ref::Url { target, .. } | Ref::Import { target, .. }) = r;
            if keep(target) {
                return Ok(None);
            }
            let url = match base.join(target.trim()) {
                Ok(u) => u,
                Err(_) => return Ok(None),
            };
            match r {
                Ref::Url { .. } => match self.load(&url).await {
                    Ok(res) => Ok(Some(format!("url(\"{}\")", res.data_url()))),
                    Err(e) => {
                        self.warn(format!("css: kept url({target}): {e:#}"));
                        Ok(None)
                    }
                },
                Ref::Import { condition, .. } => {
                    if depth >= 16 {
                        anyhow::bail!("css: @import nested too deep at {url}");
                    }
                    match self.load(&url).await {
                        Ok(res) => {
                            let text = String::from_utf8_lossy(&res.bytes).into_owned();
                            let inner = Box::pin(self.inline_css(&text, &res.url, depth + 1)).await?;
                            Ok(Some(wrap_import(&inner, condition)))
                        }
                        Err(e) => {
                            self.warn(format!("css: kept @import {target}: {e:#}"));
                            Ok(None)
                        }
                    }
                }
            }
        });
        for r in futures_util::future::join_all(loads).await {
            parts.push(r?);
        }
        let mut out = String::with_capacity(css.len());
        let mut last = 0;
        for (r, replacement) in refs.iter().zip(parts) {
            let (Ref::Url { start, end, .. } | Ref::Import { start, end, .. }) = r;
            if let Some(new) = replacement {
                out.push_str(&css[last..*start]);
                out.push_str(&new);
                last = *end;
            }
        }
        out.push_str(&css[last..]);
        Ok(out)
    }
}

/// An imported sheet in place of its `@import`, keeping `layer(...)` and media conditions.
fn wrap_import(css: &str, condition: &str) -> String {
    let mut inner = css.to_string();
    let mut cond = condition.trim();
    let mut layer = None;
    if let Some(rest) = cond.strip_prefix("layer") {
        if let Some(r) = rest.strip_prefix('(') {
            let close = r.find(')').unwrap_or(r.len());
            layer = Some(r[..close].trim().to_string());
            cond = r.get(close + 1..).unwrap_or("").trim();
        } else if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            layer = Some(String::new());
            cond = rest.trim();
        }
    }
    if let Some(s) = cond.strip_prefix("supports(") {
        let close = s.find(')').unwrap_or(s.len());
        inner = format!("@supports ({}) {{\n{inner}\n}}", s[..close].trim());
        cond = s.get(close + 1..).unwrap_or("").trim();
    }
    if !cond.is_empty() {
        inner = format!("@media {cond} {{\n{inner}\n}}");
    }
    match layer {
        Some(name) if name.is_empty() => format!("@layer {{\n{inner}\n}}"),
        Some(name) => format!("@layer {name} {{\n{inner}\n}}"),
        None => inner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_references_outside_comments_and_strings() {
        let css = r#"@import url("a.css") screen;
@import 'b.css';
/* url(skip.png) @import "skip.css"; */
.x { background: url( img/bg.png ) no-repeat; content: "url(not-this)"; }
.y { mask: URL('m\26 n.svg'); }
.z { --curl(1): 2; background-image: url(data:image/png;base64,AA==); }
@font-face { src: url(f.woff2) format("woff2"), url("f.woff") format("woff"); }"#;
        let refs = scan(css);
        let targets: Vec<&str> = refs
            .iter()
            .map(|r| match r {
                Ref::Url { target, .. } | Ref::Import { target, .. } => target.as_str(),
            })
            .collect();
        assert_eq!(
            targets,
            ["a.css", "b.css", "img/bg.png", "m&n.svg", "data:image/png;base64,AA==", "f.woff2", "f.woff"]
        );
        match &refs[0] {
            Ref::Import { condition, start, end, .. } => {
                assert_eq!(condition, "screen");
                assert_eq!(&css[*start..*end], r#"@import url("a.css") screen;"#);
            }
            other => panic!("{other:?}"),
        }
        let Ref::Url { start, end, .. } = &refs[2] else { panic!() };
        assert_eq!(&css[*start..*end], "url( img/bg.png )");
    }

    #[test]
    fn import_conditions_are_kept() {
        assert_eq!(wrap_import("a{}", ""), "a{}");
        assert_eq!(wrap_import("a{}", "print"), "@media print {\na{}\n}");
        assert_eq!(
            wrap_import("a{}", "layer(base) supports(display: grid) screen"),
            "@layer base {\n@media screen {\n@supports (display: grid) {\na{}\n}\n}\n}"
        );
        assert_eq!(wrap_import("a{}", "layer"), "@layer {\na{}\n}");
    }
}
