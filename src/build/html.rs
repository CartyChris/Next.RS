//! HTML documents: the streamed SSR shell, the single-file artifact, and the CSP header.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::config::{Compress, Csp, SecurityConfig};

pub const ROOT_ID: &str = "__forge_root";
pub const DATA_ID: &str = "__FORGE_DATA__";
/// The single-file app's module script (forge:file `saveSelf` copies it).
pub const APP_SCRIPT_ID: &str = "__forge_app";
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

/// Make CSS safe inside `<style>`: HTML ends the element at `</style` in any letter case.
/// `<\/` is a valid CSS escape for `</` inside strings and selectors.
pub fn escape_inline_style(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let mut last = 0;
    for (at, _) in css.match_indices('<') {
        let rest = &bytes[at..];
        if rest.len() >= 7 && rest[1] == b'/' && rest[2..7].eq_ignore_ascii_case(b"style") {
            out.push_str(&css[last..at]);
            out.push_str("<\\/");
            last = at + 2;
        }
    }
    out.push_str(&css[last..]);
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
    pub compress: Compress,
}

/// `"auto"` compression applies from this script size, when it saves at least a quarter.
/// Below it the readable inline script is worth more than a few kilobytes.
const AUTO_COMPRESS_MIN: usize = 16 * 1024;

/// Byte values that cannot appear raw in a JS string inside `<script>`: NUL and CR are
/// rewritten by the HTML parser, LF / `"` / `\` end or escape the string, `<` could start
/// `</script`. Everything else in 0x01..0x7f passes through both parsers unchanged.
const B122_ILLEGAL: [u8; 6] = [0, b'\n', b'\r', b'"', b'\\', b'<'];

/// Binary as text at 7 bits per byte (base64 manages 6). The input is cut into 7-bit groups;
/// a legal group is one ASCII character, an illegal one is folded with the next group into a
/// single two-byte UTF-8 character: code point `(illegal index + 1) << 7 | next group`.
/// Decoding needs the byte length: trailing padding bits are dropped.
pub fn base122(bytes: &[u8]) -> String {
    let mut groups = Vec::with_capacity(bytes.len() * 8 / 7 + 2);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 7 {
            bits -= 7;
            groups.push(((acc >> bits) & 0x7f) as u8);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        groups.push(((acc << (7 - bits)) & 0x7f) as u8);
    }
    let mut out = String::with_capacity(groups.len() + groups.len() / 8);
    let mut i = 0;
    while i < groups.len() {
        let g = groups[i];
        match B122_ILLEGAL.iter().position(|&c| c == g) {
            Some(k) => {
                let next = groups.get(i + 1).copied().unwrap_or(0);
                out.push(char::from_u32(((k as u32 + 1) << 7) | u32::from(next)).expect("below U+0400"));
                i += 2;
            }
            None => {
                out.push(char::from(g));
                i += 1;
            }
        }
    }
    out
}

/// The inverse of `base122` in JS, as a statement that defines `b` (the bytes).
fn base122_decoder(text: &str, len: usize) -> String {
    format!(
        r#"const s="{text}",I=[0,10,13,34,92,60],b=new Uint8Array({len});let a=0,k=0,j=0;const p=g=>{{a=(a<<7|g)&16383;if((k+=7)>7)b[j++]=a>>(k-=8)}};for(let i=0;i<s.length;i++){{const c=s.charCodeAt(i);c>127?(p(I[(c>>7)-1]),p(c&127)):p(c)}}"#
    )
}

/// The module `<script>` element for a single-file app, and whether it is compressed.
/// Compressed: deflate-raw, then `base122` text, inflated with `DecompressionStream` and
/// imported from a blob URL, so the code runs as the same ES module it would be inline.
pub fn single_file_script(js: &str, compress: Compress) -> (String, bool) {
    let inline = || format!(r#"<script type="module" id="{APP_SCRIPT_ID}">{}</script>"#, escape_inline_script(js));
    let packed = || {
        let bytes = deflate(js.as_bytes());
        format!(
            r#"<script type="module" id="{APP_SCRIPT_ID}">{};await import(URL.createObjectURL(new Blob([await new Response(new Blob([b]).stream().pipeThrough(new DecompressionStream("deflate-raw"))).blob()],{{type:"text/javascript"}})));</script>"#,
            base122_decoder(&base122(&bytes), bytes.len())
        )
    };
    match compress {
        Compress::Off => (inline(), false),
        Compress::On => (packed(), true),
        Compress::Auto if js.len() < AUTO_COMPRESS_MIN => (inline(), false),
        Compress::Auto => {
            let (plain, small) = (inline(), packed());
            if small.len() * 4 <= plain.len() * 3 { (small, true) } else { (plain, false) }
        }
    }
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
            let _ = write!(out, "<style>{}</style>", escape_inline_style(self.css));
        }
        let _ = write!(out, r#"<script type="application/json" id="{DATA_ID}">{}</script>"#, self.boot);
        let _ = write!(out, r#"</head><body><div id="{ROOT_ID}">{}</div>"#, self.body);
        out.push_str(&single_file_script(self.js, self.compress).0);
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
    fn inline_style_escaping() {
        let css = "a{content:'</style><script>x</script>'} b{content:'</STYLE >'} c{}";
        let out = escape_inline_style(css);
        assert!(!out.to_ascii_lowercase().contains("</style"), "{out}");
        assert_eq!(escape_inline_style("a{color:red}"), "a{color:red}");
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
            compress: Compress::Auto,
        }
        .render();
        assert!(html.contains("<style>body{color:red}</style>"));
        assert!(html.contains(r#"<div id="__forge_root"><main>hi</main></div>"#));
        assert!(html.contains(r"console.log('<\/script>')"));
        assert_eq!(html.matches("</script>").count(), 2);
    }

    /// The JS decoder, transcribed.
    fn unbase122(s: &str, len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let (mut a, mut k) = (0u32, 0u32);
        let mut put = |g: u32| {
            a = ((a << 7) | g) & 16383;
            k += 7;
            if k > 7 {
                k -= 8;
                if out.len() < len {
                    out.push((a >> k) as u8);
                }
            }
        };
        for c in s.chars() {
            let c = c as u32;
            if c > 127 {
                put(u32::from(B122_ILLEGAL[(c >> 7) as usize - 1]));
                put(c & 127);
            } else {
                put(c);
            }
        }
        out
    }

    #[test]
    fn base122_round_trips_and_stays_script_safe() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut cases: Vec<Vec<u8>> = vec![vec![], vec![0], vec![0, 0], b"</script>".to_vec(), (0..=255).collect()];
        for len in [1usize, 2, 6, 7, 8, 13, 100, 4096] {
            cases.push(
                (0..len)
                    .map(|_| {
                        x ^= x << 13;
                        x ^= x >> 7;
                        x ^= x << 17;
                        x as u8
                    })
                    .collect(),
            );
        }
        for bytes in &cases {
            let text = base122(bytes);
            assert_eq!(unbase122(&text, bytes.len()), *bytes);
            assert!(!text.chars().any(|c| matches!(c, '\0' | '\n' | '\r' | '"' | '\\' | '<')));
            assert!(text.len() <= bytes.len() * 8 / 7 + 2, "{} > 8/7 of {}", text.len(), bytes.len());
        }
    }

    #[test]
    fn auto_compression_needs_size_and_savings() {
        // Small scripts stay readable.
        let small = "console.log(1);".repeat(100);
        assert!(!single_file_script(&small, Compress::Auto).1);
        assert!(single_file_script(&small, Compress::On).1);
        // Large, repetitive code compresses well.
        let big: String = (0..4000).map(|i| format!("export const v{i} = {{ id: {i}, label: \"item\" }};\n")).collect();
        let (tag, packed) = single_file_script(&big, Compress::Auto);
        assert!(packed && tag.len() * 3 < big.len(), "{} vs {}", tag.len(), big.len());
        assert!(!single_file_script(&big, Compress::Off).1);
        // Incompressible bytes do not pay for base64.
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let noise: String = (0..40_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                char::from(b'!' + (x % 90) as u8)
            })
            .collect();
        assert!(!single_file_script(&noise, Compress::Auto).1);
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
