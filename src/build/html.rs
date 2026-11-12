//! HTML documents: the streamed SSR shell, the single-file artifact, and the CSP header.

use std::fmt::Write as _;

use base64::Engine as _;
use serde_json::{Map, Value};

use crate::config::{Csp, SecurityConfig};

pub const ROOT_ID: &str = "__forge_root";
pub const DATA_ID: &str = "__FORGE_DATA__";
pub const LIVE_RELOAD_PATH: &str = "/_forge/live.js";

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Make JS safe to inline in a `<script>` element: no `</script` (any case) and no `<!--`.
/// Both rewrites are no-ops for the JS itself (`\/` and `\!` mean `/` and `!` in strings,
/// templates and regexes, and the sequences cannot appear in code outside those).
pub fn escape_inline_script(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len() + 16);
    let mut last = 0;
    let mut i = 0;
    while let Some(off) = memchr_lt(&bytes[i..]) {
        let at = i + off;
        let rest = &bytes[at..];
        let fix = if rest.len() >= 8 && rest[1] == b'/' && rest[2..8].eq_ignore_ascii_case(b"script") {
            Some(("<\\/", 2))
        } else if rest.starts_with(b"<!--") {
            Some(("<\\!", 2))
        } else {
            None
        };
        if let Some((with, skip)) = fix {
            out.push_str(&code[last..at]);
            out.push_str(with);
            last = at + skip;
        }
        i = at + 1;
    }
    out.push_str(&code[last..]);
    out
}

fn memchr_lt(b: &[u8]) -> Option<usize> {
    b.iter().position(|&c| c == b'<')
}

/// JSON safe inside `<script type="application/json">`.
pub fn script_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into()).replace('<', "\\u003c")
}

/// `<title>` and friends from a page's resolved `meta` export.
pub fn meta_tags(meta: &Map<String, Value>, default_title: &str) -> String {
    let str_of = |k: &str| meta.get(k).and_then(Value::as_str);
    let mut out = format!("<title>{}</title>", escape_html(str_of("title").unwrap_or(default_title)));
    let mut tag = |attr: &str, name: &str, content: Option<&str>| {
        if let Some(c) = content {
            let _ = write!(out, r#"<meta {attr}="{name}" content="{}">"#, escape_html(c));
        }
    };
    tag("name", "description", str_of("description"));
    tag("name", "theme-color", str_of("themeColor"));
    tag("name", "robots", str_of("robots"));
    tag("property", "og:title", str_of("title"));
    tag("property", "og:description", str_of("description"));
    tag("property", "og:image", str_of("image"));
    if let Some(icon) = str_of("icon") {
        let _ = write!(out, r#"<link rel="icon" href="{}">"#, escape_html(icon));
    }
    if let Some(c) = str_of("canonical") {
        let _ = write!(out, r#"<link rel="canonical" href="{}">"#, escape_html(c));
    }
    out
}

pub struct Document<'a> {
    pub lang: &'a str,
    pub head_tags: String,
    pub css_href: Option<&'a str>,
    pub script_src: &'a str,
    pub preload: &'a [String],
    pub boot: &'a str,
    pub nonce: Option<&'a str>,
    pub live_reload: bool,
}

impl Document<'_> {
    /// Everything before the app markup. The client script is a deferred module in the head,
    /// so it runs after the whole (possibly streamed) body has been parsed.
    pub fn open(&self) -> String {
        let mut out = String::with_capacity(1024);
        let _ = write!(
            out,
            r#"<!doctype html><html lang="{}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{}"#,
            escape_html(self.lang),
            self.head_tags
        );
        if let Some(css) = self.css_href {
            let _ = write!(out, r#"<link rel="stylesheet" href="{css}">"#);
        }
        for href in self.preload {
            let _ = write!(out, r#"<link rel="modulepreload" href="{href}">"#);
        }
        let _ = write!(out, r#"<script type="application/json" id="{DATA_ID}">{}</script>"#, self.boot);
        let _ = write!(out, r#"<script type="module" src="{}"></script>"#, self.script_src);
        if self.live_reload {
            let _ = write!(out, r#"<script type="module" src="{LIVE_RELOAD_PATH}"></script>"#);
        }
        let _ = write!(out, r#"</head><body><div id="{ROOT_ID}">"#);
        out
    }

    pub fn close() -> &'static str {
        "</div></body></html>"
    }
}

pub struct SingleFile<'a> {
    pub lang: &'a str,
    pub head_tags: String,
    pub css: &'a str,
    pub js: &'a str,
    pub boot: &'a str,
    pub body: &'a str,
    /// Ship the script deflated and inflate it in the browser.
    pub compress: bool,
}

impl SingleFile<'_> {
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.js.len() + self.css.len() + self.body.len() + 1024);
        let _ = write!(
            out,
            r#"<!doctype html><html lang="{}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">{}"#,
            escape_html(self.lang),
            self.head_tags
        );
        if !self.css.is_empty() {
            let _ = write!(out, "<style>{}</style>", self.css.replace("</style", "<\\/style"));
        }
        let _ = write!(out, r#"<script type="application/json" id="{DATA_ID}">{}</script>"#, self.boot);
        let _ = write!(out, r#"</head><body><div id="{ROOT_ID}">{}</div>"#, self.body);
        if self.compress {
            let packed = deflate(self.js.as_bytes());
            let b64 = base64::engine::general_purpose::STANDARD.encode(packed);
            let _ = write!(
                out,
                r#"<script type="module">const b=Uint8Array.from(atob("{b64}"),c=>c.charCodeAt(0));const s=await new Response(new Blob([b]).stream().pipeThrough(new DecompressionStream("deflate-raw"))).text();await import(URL.createObjectURL(new Blob([s],{{type:"text/javascript"}})));</script>"#
            );
        } else {
            let _ = write!(out, r#"<script type="module">{}</script>"#, escape_inline_script(self.js));
        }
        out.push_str("</body></html>");
        out
    }
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(bytes).expect("in-memory write");
    enc.finish().expect("in-memory write")
}

/// The Content-Security-Policy for a response, or `None` when disabled.
pub fn csp(security: &SecurityConfig, nonce: &str, dev: bool) -> Option<String> {
    match &security.csp {
        Some(Csp::Enabled(false)) => return None,
        Some(Csp::Custom(s)) => return Some(s.replace("{nonce}", nonce)),
        _ => {}
    }
    let eval = if security.allow_eval { " 'unsafe-eval'" } else { "" };
    let mut connect = String::from("'self'");
    for origin in &security.connect_src {
        connect.push(' ');
        connect.push_str(origin);
    }
    if dev {
        connect.push_str(" ws: wss:");
    }
    Some(format!(
        "default-src 'self'; script-src 'self' 'nonce-{nonce}' 'wasm-unsafe-eval'{eval}; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; media-src 'self' data: blob:; font-src 'self' data:; connect-src {connect}; \
         worker-src 'self' blob:; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'self'"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_script_escaping() {
        assert_eq!(escape_inline_script(r#"a="</script>";b="</SCRIPT >""#), r#"a="<\/script>";b="<\/SCRIPT >""#);
        assert_eq!(escape_inline_script("x='<!-- y'"), r"x='<\!-- y'");
        assert_eq!(escape_inline_script("if (a<b) c</scrip;"), "if (a<b) c</scrip;");
        assert_eq!(escape_inline_script("ünïcödé</script"), r"ünïcödé<\/script");
    }

    #[test]
    fn script_json_cannot_close_the_tag() {
        let v = serde_json::json!({"x": "</script><script>alert(1)</script>"});
        assert!(!script_json(&v).contains("</"));
    }

    #[test]
    fn meta_is_escaped() {
        let meta: Map<String, Value> = serde_json::from_str(r#"{"title":"a <b> & \"c\"","description":"d"}"#).unwrap();
        let tags = meta_tags(&meta, "x");
        assert!(tags.starts_with("<title>a &lt;b&gt; &amp; &quot;c&quot;</title>"));
        assert!(tags.contains(r#"<meta name="description" content="d">"#));
    }

    #[test]
    fn single_file_document() {
        let html = SingleFile {
            lang: "en",
            head_tags: "<title>t</title>".into(),
            css: "body{color:red}",
            js: "console.log('</script>')",
            boot: "{}",
            body: "<main>hi</main>",
            compress: false,
        }
        .render();
        assert!(html.contains("<style>body{color:red}</style>"));
        assert!(html.contains(r#"<div id="__forge_root"><main>hi</main></div>"#));
        assert!(html.contains(r"console.log('<\/script>')"));
        assert_eq!(html.matches("</script>").count(), 2);
    }

    #[test]
    fn csp_modes() {
        let mut sec = SecurityConfig::default();
        let p = csp(&sec, "abc", false).unwrap();
        assert!(p.contains("'nonce-abc'") && !p.contains("'unsafe-eval'") && p.contains("'wasm-unsafe-eval'"));
        sec.allow_eval = true;
        sec.connect_src = vec!["https://api.openai.com".into()];
        let p = csp(&sec, "abc", false).unwrap();
        assert!(p.contains("'unsafe-eval'") && p.contains("connect-src 'self' https://api.openai.com"));
        sec.csp = Some(Csp::Enabled(false));
        assert!(csp(&sec, "abc", false).is_none());
    }
}
