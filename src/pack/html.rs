//! The page rewrite. One scan records every element; everything they reference is loaded
//! concurrently; one rewrite applies the results.
//!
//! * module scripts (+ import map) → one bundled module, compressed like single-html builds
//! * classic scripts and stylesheets → inline, compressed when large (they still run in place)
//! * `text/babel` → compiled ahead of time; the Babel runtime is dropped
//! * images, icons, media, fonts, `url()`s and asset URLs in page code → data URLs
//! * resource hints, `nomodule` fallbacks and CSP `<meta>` (it would block inline code) → removed

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use anyhow::{Result, anyhow, bail};
use lol_html::{RewriteStrSettings, element, html_content::ContentType, rewrite_str, text};
use url::Url;

use super::{Packer, inline, js};
use crate::build::html::{escape_html, escape_inline_script, escape_inline_style, single_file_script};

/// Larger embedded media is left as a link, with a warning.
const MAX_EMBED: usize = 32 << 20;

#[derive(Debug, Default)]
struct El {
    tag: String,
    attrs: Vec<(String, String)>,
    text: String,
}

impl El {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn script_type(&self) -> String {
        self.attr("type").unwrap_or("").trim().to_ascii_lowercase()
    }
    fn rel(&self) -> Vec<String> {
        self.attr("rel").unwrap_or("").split_ascii_whitespace().map(str::to_ascii_lowercase).collect()
    }
}

fn scan(html: &str) -> Result<Vec<El>> {
    let els = RefCell::new(Vec::<El>::new());
    let current = RefCell::new(None::<usize>);
    rewrite_str(
        html,
        RewriteStrSettings::new()
            .append_element_content_handler(element!("*", |el| {
                let tag = el.tag_name().to_ascii_lowercase();
                let attrs = el.attributes().iter().map(|a| (a.name().to_ascii_lowercase(), a.value())).collect();
                let mut v = els.borrow_mut();
                *current.borrow_mut() = matches!(tag.as_str(), "script" | "style").then_some(v.len());
                v.push(El { tag, attrs, text: String::new() });
                Ok(())
            }))
            .append_element_content_handler(text!("script, style", |t| {
                if let Some(i) = *current.borrow() {
                    els.borrow_mut()[i].text.push_str(t.as_str());
                }
                Ok(())
            })),
    )
    .map_err(|e| anyhow!("parsing the page: {e}"))?;
    Ok(els.into_inner())
}

#[derive(Debug, Clone)]
enum Edit {
    Remove,
    /// Replace the element (and its content) with HTML.
    Replace(String),
    /// Raw content (script / style text).
    Inner(String),
    Set(String, String),
    Unset(String),
}

/// What one element turns into, plus content for the end of `<body>` (deferred scripts).
#[derive(Default)]
struct Out {
    edits: Vec<Edit>,
    tail: Option<String>,
    /// Uses the inflater: it must be defined before the first such element.
    inflates: bool,
}

impl Out {
    fn edit(edit: Edit) -> Self {
        Self { edits: vec![edit], ..Default::default() }
    }
}

const DROP_ATTRS: &[&str] = &[
    "src",
    "href",
    "integrity",
    "crossorigin",
    "referrerpolicy",
    "async",
    "defer",
    "type",
    "rel",
    "as",
    "fetchpriority",
    "blocking",
];

fn kept_attrs(el: &El, drop_extra: &[&str]) -> Vec<(String, String)> {
    el.attrs
        .iter()
        .filter(|(k, _)| !DROP_ATTRS.contains(&k.as_str()) && !drop_extra.contains(&k.as_str()))
        .cloned()
        .collect()
}

fn attrs_html(attrs: &[(String, String)]) -> String {
    attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", escape_html(v))).collect()
}

/// A classic `<script>` (compressed when it pays off) with `attrs`.
fn classic_script(p: &Packer, attrs: &[(String, String)], code: &str) -> (String, bool) {
    match inline::pack(code, p.options.compress) {
        Some(packed) => (inline::loader("script", attrs, &packed), true),
        None => (format!("<script{}>{}</script>", attrs_html(attrs), escape_inline_script(code)), false),
    }
}

fn style_element(p: &Packer, attrs: &[(String, String)], css: &str) -> (String, bool) {
    match inline::pack(css, p.options.compress) {
        Some(packed) => (inline::loader("style", attrs, &packed), true),
        None => (format!("<style{}>{}</style>", attrs_html(attrs), escape_inline_style(css)), false),
    }
}

fn is_babel_runtime(url: &Url) -> bool {
    let s = url.as_str().to_ascii_lowercase();
    s.contains("@babel/standalone")
        || s.contains("babel-standalone")
        || s.ends_with("/babel.min.js")
        || s.ends_with("/babel.js")
}

/// `srcset` → the largest candidate (`w`, then `x` descriptors).
fn best_candidate(srcset: &str) -> Option<String> {
    srcset
        .split(", ")
        .filter_map(|c| {
            let mut parts = c.split_whitespace();
            let url = parts.next()?.trim_end_matches(',').to_string();
            let d = parts.next().unwrap_or("1x");
            let n: f64 = d.trim_end_matches(['w', 'x']).parse().unwrap_or(1.0);
            Some((if d.ends_with('w') { n } else { n * 1000.0 }, url))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, u)| u)
}

/// URL-valued attributes embedded as data URLs, by element.
fn media_attrs(tag: &str) -> &'static [&'static str] {
    match tag {
        "img" | "audio" | "track" | "embed" | "input" | "iframe-not" => &["src"],
        "video" => &["src", "poster"],
        "source" => &["src"],
        "object" => &["data"],
        "image" => &["href", "xlink:href"],
        _ => &[],
    }
}

pub async fn pack(p: &Arc<Packer>, html: &str) -> Result<String> {
    let els = scan(html)?;
    let base = els
        .iter()
        .find(|e| e.tag == "base")
        .and_then(|e| e.attr("href"))
        .and_then(|h| p.page.join(h).ok())
        .unwrap_or_else(|| p.page.clone());
    let resolve = |s: &str| -> Option<Url> {
        let s = s.trim();
        let skip = ["data:", "blob:", "javascript:", "mailto:", "about:", "#"];
        if s.is_empty() || skip.iter().any(|x| s.to_ascii_lowercase().starts_with(x)) {
            return None;
        }
        if s.starts_with("//") {
            return Url::parse(&format!("https:{s}")).ok();
        }
        base.join(s).ok()
    };

    // Module scripts and the import map are handled together; text/babel needs a decision
    // about the Babel runtime before the rest.
    let import_map = match els.iter().find(|e| e.tag == "script" && e.script_type() == "importmap") {
        Some(e) => Some(js::ImportMap::parse(&e.text, &base)?),
        None => None,
    };
    let babel =
        |e: &El| e.tag == "script" && matches!(e.script_type().as_str(), "text/babel" | "text/jsx" | "text/tsx");
    let has_babel = els.iter().any(babel);

    let mut outs: HashMap<usize, Out> = HashMap::new();
    let mut modules: Vec<js::ModuleInput> = Vec::new();
    let mut first_module = None;
    let mut jobs = Vec::new();

    for (i, el) in els.iter().enumerate() {
        match el.tag.as_str() {
            "meta" if el.attr("http-equiv").is_some_and(|h| h.eq_ignore_ascii_case("content-security-policy")) => {
                p.warn("removed a Content-Security-Policy <meta>: it would block the inlined code");
                outs.insert(i, Out::edit(Edit::Remove));
            }
            // The output is UTF-8 and says so first thing (see `rewrite`).
            "meta"
                if el.attr("charset").is_some()
                    || el.attr("http-equiv").is_some_and(|h| h.eq_ignore_ascii_case("content-type"))
                    || el.attr("name").is_some_and(|n| n.eq_ignore_ascii_case("generator")) =>
            {
                outs.insert(i, Out::edit(Edit::Remove));
            }
            "script" => {
                let t = el.script_type();
                if t == "importmap" {
                    outs.insert(i, Out::edit(Edit::Remove));
                } else if t == "module" {
                    first_module.get_or_insert(i);
                    match el.attr("src").and_then(resolve) {
                        Some(url) => modules.push(js::ModuleInput::Url(url)),
                        None if el.attr("src").is_some() => {}
                        None => modules.push(js::ModuleInput::Inline(el.text.clone())),
                    }
                    if first_module != Some(i) {
                        outs.insert(i, Out::edit(Edit::Remove));
                    }
                } else if babel(el) {
                    jobs.push(Job::Babel(i));
                } else if el.attr("nomodule").is_some() {
                    outs.insert(i, Out::edit(Edit::Remove));
                } else if matches!(
                    t.as_str(),
                    "" | "text/javascript" | "application/javascript" | "text/ecmascript" | "application/ecmascript"
                ) {
                    match el.attr("src") {
                        Some(src) => match resolve(src) {
                            Some(url) if has_babel && is_babel_runtime(&url) => {
                                p.note("dropped the Babel runtime: text/babel scripts are compiled ahead of time");
                                outs.insert(i, Out::edit(Edit::Remove));
                            }
                            Some(url) => jobs.push(Job::Script(i, url)),
                            None => {}
                        },
                        None => jobs.push(Job::InlineScript(i)),
                    }
                }
            }
            "link" => {
                let rel = el.rel();
                let href = el.attr("href").and_then(resolve);
                if rel.iter().any(|r| r == "stylesheet") && !rel.iter().any(|r| r == "alternate") {
                    if let Some(url) = href {
                        jobs.push(Job::Stylesheet(i, url));
                    }
                } else if rel.iter().any(|r| {
                    matches!(
                        r.as_str(),
                        "preload" | "modulepreload" | "prefetch" | "preconnect" | "dns-prefetch" | "manifest"
                    )
                }) {
                    outs.insert(i, Out::edit(Edit::Remove));
                } else if rel.iter().any(|r| r.contains("icon")) {
                    if let Some(url) = href {
                        jobs.push(Job::Attr(i, "href", url));
                    }
                }
            }
            "style" => jobs.push(Job::Style(i)),
            tag => {
                for name in media_attrs(tag) {
                    if tag == "input" && !el.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("image")) {
                        continue;
                    }
                    if let Some(url) = el.attr(name).and_then(resolve) {
                        jobs.push(Job::Attr(i, name, url));
                    }
                }
                if let Some(srcset) = el.attr("srcset") {
                    jobs.push(Job::Srcset(i, srcset.to_string()));
                }
            }
        }
        if el.attr("style").is_some_and(|s| s.to_ascii_lowercase().contains("url(")) {
            jobs.push(Job::StyleAttr(i));
        }
    }

    let results = futures_util::future::join_all(jobs.into_iter().map(|job| run(p, &els, &base, &resolve, job))).await;
    for r in results {
        let (i, out) = r?;
        let entry = outs.entry(i).or_default();
        entry.edits.extend(out.edits);
        entry.inflates |= out.inflates;
        if out.tail.is_some() {
            entry.tail = out.tail;
        }
    }

    if let Some(i) = first_module {
        let bundled = p.bundle_modules(modules, import_map, &base).await?;
        let mut html = String::new();
        if !bundled.css.trim().is_empty() {
            html.push_str(&style_element(p, &[], &bundled.css).0);
        }
        let (script, compressed) = single_file_script(&bundled.js, p.options.compress, None);
        html.push_str(&script);
        p.note(format!(
            "bundled the page's module scripts: {} kB{}",
            bundled.js.len() / 1024,
            if compressed { ", compressed" } else { "" }
        ));
        outs.insert(i, Out::edit(Edit::Replace(html)));
    }

    // The inflater goes with the first element that needs it.
    let mut tail = String::new();
    let mut order: Vec<usize> = outs.keys().copied().collect();
    order.sort_unstable();
    let first_inflate = order.iter().copied().find(|i| outs[i].inflates && outs[i].tail.is_none());
    let tail_inflates = order.iter().any(|i| outs[i].inflates && outs[i].tail.is_some());
    let runtime = format!("<script>{}</script>", inline::INFLATE_JS);
    match first_inflate {
        Some(i) => {
            let out = outs.get_mut(&i).unwrap();
            match out.edits.iter_mut().find(|e| matches!(e, Edit::Replace(_))) {
                Some(Edit::Replace(h)) => h.insert_str(0, &runtime),
                _ => unreachable!("inflating elements are replaced"),
            }
        }
        None if tail_inflates => tail.push_str(&runtime),
        None => {}
    }
    for i in &order {
        if let Some(t) = &outs[i].tail {
            tail.push_str(t);
        }
    }
    rewrite(html, &els, outs, &tail)
}

enum Job {
    Script(usize, Url),
    InlineScript(usize),
    Babel(usize),
    Stylesheet(usize, Url),
    Style(usize),
    StyleAttr(usize),
    Attr(usize, &'static str, Url),
    Srcset(usize, String),
}

async fn run(
    p: &Arc<Packer>,
    els: &[El],
    base: &Url,
    resolve: &impl Fn(&str) -> Option<Url>,
    job: Job,
) -> Result<(usize, Out)> {
    let warn_keep = |what: &str, url: &Url, e: anyhow::Error| p.warn(format!("kept {what} {url} as a link: {e:#}"));
    Ok(match job {
        Job::Script(i, url) => {
            let el = &els[i];
            let res = match p.load(&url).await {
                Ok(r) => r,
                Err(e) => {
                    warn_keep("script", &url, e);
                    return Ok((i, Out::default()));
                }
            };
            if let Some(sri) = el.attr("integrity") {
                p.check_integrity(&res, sri)?;
            }
            let mut code = res.text();
            if res.url.scheme() == "file" && res.url.to_file_path().is_ok_and(|f| f.starts_with(&p.dir)) {
                code = p.sweep(&code, oxc::span::SourceType::cjs(), base, &res.url).await?.unwrap_or(code);
            }
            let (html, inflates) = classic_script(p, &kept_attrs(el, &[]), &code);
            let deferred = el.attr("defer").is_some() || el.attr("async").is_some();
            if deferred {
                (i, Out { edits: vec![Edit::Remove], tail: Some(html), inflates })
            } else {
                (i, Out { edits: vec![Edit::Replace(html)], tail: None, inflates })
            }
        }
        Job::InlineScript(i) => {
            let el = &els[i];
            let code =
                p.sweep(&el.text, oxc::span::SourceType::cjs(), base, base).await?.unwrap_or_else(|| el.text.clone());
            let (html, inflates) = classic_script(p, &kept_attrs(el, &[]), &code);
            if inflates {
                (i, Out { edits: vec![Edit::Replace(html)], tail: None, inflates })
            } else if code != el.text {
                (i, Out::edit(Edit::Inner(escape_inline_script(&code))))
            } else {
                (i, Out::default())
            }
        }
        Job::Babel(i) => {
            let el = &els[i];
            let (code, url) = match el.attr("src").and_then(resolve) {
                Some(url) => (p.load(&url).await?.text(), url),
                None => (el.text.clone(), base.clone()),
            };
            let ts =
                el.script_type() == "text/tsx" || el.attr("data-presets").is_some_and(|v| v.contains("typescript"));
            let module = el.attr("data-type").is_some_and(|t| t == "module");
            if module {
                bail!(
                    "text/babel with data-type=\"module\" is not supported yet; use a plain <script type=\"module\"> with an import map"
                );
            }
            let compiled = js::compile_jsx(&code, ts, false)?;
            let compiled = p.sweep(&compiled, oxc::span::SourceType::cjs(), base, &url).await?.unwrap_or(compiled);
            let (html, inflates) =
                classic_script(p, &kept_attrs(el, &["data-presets", "data-type", "data-plugins"]), &compiled);
            p.note("compiled a text/babel script ahead of time");
            // Babel runs these once the document is parsed: the end of <body> keeps that order.
            (i, Out { edits: vec![Edit::Remove], tail: Some(html), inflates })
        }
        Job::Stylesheet(i, url) => {
            let el = &els[i];
            let res = match p.load(&url).await {
                Ok(r) => r,
                Err(e) => {
                    warn_keep("stylesheet", &url, e);
                    return Ok((i, Out::default()));
                }
            };
            if let Some(sri) = el.attr("integrity") {
                p.check_integrity(&res, sri)?;
            }
            let css = p.inline_css(&res.text(), &res.url, 0).await?;
            // `media="print" onload="this.media='all'"` loads CSS without blocking: inline, it applies now.
            let lazy = el.attr("onload").is_some_and(|o| o.contains("media"));
            let attrs = kept_attrs(el, if lazy { &["media", "onload"] } else { &["onload"] });
            let (html, inflates) = style_element(p, &attrs, &css);
            (i, Out { edits: vec![Edit::Replace(html)], tail: None, inflates })
        }
        Job::Style(i) => {
            let el = &els[i];
            let css = p.inline_css(&el.text, base, 0).await?;
            let (html, inflates) = style_element(p, &kept_attrs(el, &[]), &css);
            if inflates {
                (i, Out { edits: vec![Edit::Replace(html)], tail: None, inflates })
            } else if css != el.text {
                (i, Out::edit(Edit::Inner(escape_inline_style(&css))))
            } else {
                (i, Out::default())
            }
        }
        Job::StyleAttr(i) => {
            let style = els[i].attr("style").unwrap_or_default();
            let css = p.inline_css(style, base, 0).await?;
            (i, if css != style { Out::edit(Edit::Set("style".into(), css)) } else { Out::default() })
        }
        Job::Attr(i, name, url) => match p.load(&url).await {
            Ok(res) if res.bytes.len() <= MAX_EMBED => (i, Out::edit(Edit::Set(name.into(), res.data_url()))),
            Ok(res) => {
                p.warn(format!("kept {url} as a link: {} MB is too large to embed", res.bytes.len() >> 20));
                (i, Out::default())
            }
            Err(e) => {
                warn_keep(&els[i].tag, &url, e);
                (i, Out::default())
            }
        },
        Job::Srcset(i, srcset) => {
            let el = &els[i];
            // With a `src`, that is the image; otherwise the largest candidate becomes it.
            if el.attr("src").is_some() && el.tag == "img" {
                return Ok((
                    i,
                    Out {
                        edits: vec![Edit::Unset("srcset".into()), Edit::Unset("sizes".into())],
                        ..Default::default()
                    },
                ));
            }
            let Some(url) = best_candidate(&srcset).and_then(|u| resolve(&u)) else { return Ok((i, Out::default())) };
            match p.load(&url).await {
                Ok(res) => (
                    i,
                    Out {
                        edits: vec![Edit::Set("srcset".into(), res.data_url()), Edit::Unset("sizes".into())],
                        ..Default::default()
                    },
                ),
                Err(e) => {
                    warn_keep("image", &url, e);
                    (i, Out::default())
                }
            }
        }
    })
}

/// Without a declared charset a `file://` page is read as windows-1252, which garbles any
/// non-ASCII text (and compressed payloads); the declaration must be in the first 1 kB.
const CHARSET: &str = r#"<meta charset="utf-8"><meta name="generator" content="forge pack">"#;
/// Marks files `forge pack` wrote (and may overwrite).
pub const GENERATOR: &str = r#"<meta name="generator" content="forge pack">"#;

fn rewrite(html: &str, _els: &[El], outs: HashMap<usize, Out>, tail: &str) -> Result<String> {
    let index = RefCell::new(0usize);
    let outs = RefCell::new(outs);
    let tail_done = RefCell::new(tail.is_empty());
    let charset_done = RefCell::new(false);
    let out = rewrite_str(
        html,
        RewriteStrSettings::new().append_element_content_handler(element!("*", |el| {
            let i = {
                let mut n = index.borrow_mut();
                *n += 1;
                *n - 1
            };
            if !*charset_done.borrow() && matches!(el.tag_name().to_ascii_lowercase().as_str(), "html" | "head") {
                el.prepend(CHARSET, ContentType::Html);
                *charset_done.borrow_mut() = true;
            }
            if el.tag_name().eq_ignore_ascii_case("body") && !*tail_done.borrow() {
                el.append(tail, ContentType::Html);
                *tail_done.borrow_mut() = true;
            }
            if let Some(out) = outs.borrow_mut().remove(&i) {
                for edit in out.edits {
                    match edit {
                        Edit::Remove => el.remove(),
                        Edit::Replace(h) => el.replace(&h, ContentType::Html),
                        Edit::Inner(h) => el.set_inner_content(&h, ContentType::Html),
                        Edit::Set(k, v) => el.set_attribute(&k, &v)?,
                        Edit::Unset(k) => el.remove_attribute(&k),
                    }
                }
            }
            Ok(())
        })),
    )
    .map_err(|e| anyhow!("rewriting the page: {e}"))?;
    let mut out = if *tail_done.borrow() {
        out
    } else {
        // No <body> tag in the source: deferred scripts go at the very end.
        format!("{out}{tail}")
    };
    if !*charset_done.borrow() {
        // No <html> or <head> tag: right after the doctype, or first.
        let at = if out.trim_start().to_ascii_lowercase().starts_with("<!doctype") {
            out.find('>').map_or(0, |i| i + 1)
        } else {
            0
        };
        out.insert_str(at, CHARSET);
    }
    Ok(out)
}

/// The page as text: UTF-8 (with or without a BOM), else the charset it declares, else
/// windows-1252 (what browsers assume).
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    let label = head.split("charset=").nth(1).map(|r| {
        r.trim_start_matches(['"', '\''])
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .next()
            .unwrap_or("")
    });
    let encoding =
        label.and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes())).unwrap_or(encoding_rs::WINDOWS_1252);
    encoding.decode(bytes).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_elements_and_script_text() {
        let els = scan(r#"<!doctype html><html><head><script type="importmap">{"imports":{}}</script><style>a{}</style></head><body><img src="x.png"><script>let a = "</div>";</script></body></html>"#).unwrap();
        let tags: Vec<&str> = els.iter().map(|e| e.tag.as_str()).collect();
        assert_eq!(tags, ["html", "head", "script", "style", "body", "img", "script"]);
        assert_eq!(els[2].text, r#"{"imports":{}}"#);
        assert_eq!(els[3].text, "a{}");
        assert_eq!(els[6].text, r#"let a = "</div>";"#);
        assert_eq!(els[5].attr("src"), Some("x.png"));
    }

    #[test]
    fn srcset_candidates() {
        assert_eq!(best_candidate("a.png 1x, b.png 2x").as_deref(), Some("b.png"));
        assert_eq!(best_candidate("s.jpg 480w, l.jpg 1080w, m.jpg 800w").as_deref(), Some("l.jpg"));
        assert_eq!(best_candidate("only.png").as_deref(), Some("only.png"));
    }

    #[test]
    fn rewrite_applies_edits_by_element_order() {
        let html = r#"<html><head><link rel="preload" href="x"><style>a{}</style></head><body><img src="a.png" srcset="b.png 2x"><p>keep</p></body></html>"#;
        let els = scan(html).unwrap();
        let mut outs = HashMap::new();
        outs.insert(2, Out::edit(Edit::Remove));
        outs.insert(3, Out::edit(Edit::Inner("b{}".into())));
        outs.insert(
            5,
            Out {
                edits: vec![Edit::Set("src".into(), "data:x".into()), Edit::Unset("srcset".into())],
                ..Default::default()
            },
        );
        let out = rewrite(html, &els, outs, "<script>tail()</script>").unwrap();
        assert_eq!(
            out,
            r#"<html><meta charset="utf-8"><meta name="generator" content="forge pack"><head><style>b{}</style></head><body><img src="data:x"><p>keep</p><script>tail()</script></body></html>"#
        );
        let bare = rewrite("<!doctype html><p>x</p>", &[], HashMap::new(), "").unwrap();
        assert_eq!(bare, format!("<!doctype html>{CHARSET}<p>x</p>"));
        let head_only = rewrite("<head><title>t</title></head>", &[], HashMap::new(), "").unwrap();
        assert_eq!(head_only, format!("<head>{CHARSET}<title>t</title></head>"));
    }

    #[test]
    fn decodes_legacy_charsets() {
        assert_eq!(decode("héllo".as_bytes()), "héllo");
        assert_eq!(decode(b"\xEF\xBB\xBFbom"), "bom");
        assert_eq!(
            decode(b"<meta charset=\"windows-1252\"><p>caf\xe9 \x80</p>"),
            "<meta charset=\"windows-1252\"><p>café €</p>"
        );
        assert_eq!(
            decode(b"<meta http-equiv=content-type content='text/html; charset=iso-8859-15'>\xa4"),
            "<meta http-equiv=content-type content='text/html; charset=iso-8859-15'>€"
        );
    }
}
