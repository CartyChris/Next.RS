//! The request protocol between Rust and `runtime/server.js`, and the page document
//! wrapper shared by the HTTP server and build-time prerendering.

use serde::Serialize;

use crate::{
    build::{bundle::STATIC_PREFIX, html, manifest::Manifest},
    routes::Params,
    runtime::{Body, DispatchError, Pool},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request<'a> {
    pub kind: &'a str,
    pub route: &'a str,
    pub params: serde_json::Map<String, serde_json::Value>,
    pub pathname: &'a str,
    pub url: &'a str,
    pub method: &'a str,
    pub headers: &'a [(String, String)],
    pub nonce: &'a str,
    pub stream: bool,
}

pub fn params_json(params: &Params) -> serde_json::Map<String, serde_json::Value> {
    params.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect()
}

pub enum Rendered {
    /// An HTML document: send `open`, stream `body`, send [`html::Document::close`].
    Page { status: u16, open: String, body: Body },
    /// Anything else (redirects, API-style responses, the built-in 404).
    Raw { status: u16, headers: Vec<(String, String)>, body: Body, builtin_not_found: bool },
}

pub struct PageOptions<'a> {
    pub nonce: Option<&'a str>,
    pub live_reload: bool,
}

/// Render a page route. The body streams; nothing is buffered here.
pub async fn page(
    pool: &Pool,
    manifest: &Manifest,
    req: &Request<'_>,
    opts: &PageOptions<'_>,
) -> Result<Rendered, DispatchError> {
    let reply = pool.dispatch(serde_json::to_string(req).expect("request serializes"), None).await?;
    let Some(page) = reply.head.page else {
        return Ok(Rendered::Raw {
            status: reply.head.status,
            headers: reply.head.headers,
            body: reply.body,
            builtin_not_found: reply.head.builtin.as_deref() == Some("not-found"),
        });
    };
    let preload: Vec<String> = manifest.client.preload.get(&page.route).cloned().unwrap_or_default();
    let doc = html::Document {
        lang: &manifest.lang,
        head_tags: html::meta_tags(&page.meta, &manifest.title),
        css_href: manifest.client.css.as_deref(),
        script_src: &manifest.client.entry,
        preload: &preload,
        boot: &page.boot,
        nonce: opts.nonce,
        live_reload: opts.live_reload,
    };
    Ok(Rendered::Page { status: reply.head.status, open: doc.open(), body: reply.body })
}

/// Minimal page for errors and the built-in 404.
pub fn simple_page(title: &str, message: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{t}</title>\
         <style>body{{font:16px/1.5 system-ui,sans-serif;margin:0;display:grid;place-items:center;min-height:100vh;background:#0b0d10;color:#e8eaed}}\
         main{{max-width:60rem;padding:2rem}}pre{{white-space:pre-wrap;background:#16191d;padding:1rem;border-radius:8px;overflow:auto;font-size:13px}}</style></head>\
         <body><main><h1>{t}</h1>{m}</main></body></html>",
        t = html::escape_html(title),
        m = message
    )
}

pub fn static_url(file: &str) -> String {
    format!("{STATIC_PREFIX}{file}")
}
