//! React, JSX and three.js examples. Needs `npm install` in each example:
//! `cargo test --features e2e` (CI does this; see .github/workflows/ci.yml).
#![cfg(feature = "e2e")]

mod common;

use axum::http::StatusCode;
use common::*;
use nextrs::config::Target;

fn require_modules(name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join(name).join("node_modules");
    assert!(dir.is_dir(), "run `npm install` in examples/{name} first");
}

#[tokio::test(flavor = "multi_thread")]
async fn react_ssr_streaming_and_data() {
    require_modules("react");
    let root = example("react");
    let app = serve(&root).await;

    let home = get(&app, "/").await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body.contains(r#"<header><strong>Forge</strong><a href="/">Home</a>"#), "layout rendered");
    assert!(
        home.body.contains(r#"<button id="inc">Clicked <!-- -->0<!-- --> times</button>"#),
        "page rendered by React"
    );
    assert!(home.body.contains(r#"<link rel="modulepreload""#));

    // Suspense: fallback first, then the resolved segment and React's nonce'd swap script.
    let slow = get(&app, "/slow").await;
    let nonce = slow.headers["content-security-policy"]
        .to_str()
        .unwrap()
        .split("'nonce-")
        .nth(1)
        .unwrap()
        .split('\'')
        .next()
        .unwrap()
        .to_string();
    let fallback = slow.body.find(r#"<p id="fallback">"#).expect("fallback streamed");
    let resolved = slow.body.find(r#"<p id="slow">"#).expect("content streamed");
    assert!(fallback < resolved);
    assert!(slow.body.contains(&format!(r#"<script nonce="{nonce}">"#)));

    let post = get(&app, "/blog/streaming").await;
    assert!(post.body.contains("<h1>Streaming SSR</h1>"));
    assert!(post.body.contains("<title>Streaming SSR · Blog</title>"));
    assert_eq!(get(&app, "/blog/missing").await.status, StatusCode::NOT_FOUND);
    assert!(get(&app, "/_forge/data?path=/blog").await.body.contains(r#""slug":"hello""#));
    assert_eq!(get(&app, "/api/hello?name=rust").await.body, r#"{"hello":"rust"}"#);

    // Crawlers get complete HTML (no streaming placeholders).
    let bot = request(&app, axum::http::Method::GET, "/slow", &[("user-agent", "Googlebot/2.1")], "").await;
    assert!(bot.body.contains(r#"<p id="slow">"#) && !bot.body.contains(r#"id="fallback""#));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn react_single_html_and_static() {
    require_modules("react");
    let root = example("react");
    build(&root, Target::SingleHtml).await.unwrap();
    let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(html.contains("<h1>Hello Forge</h1>"));
    assert!(html.contains(r#"Single-file"#) || html.contains("single-file"), "blog loader data embedded");
    build(&root, Target::Static).await.unwrap();
    for f in ["index.html", "blog/index.html", "blog/hello/index.html", "_forge/data/blog/hello.json", "404.html"] {
        assert!(root.join("dist").join(f).is_file(), "missing {f}");
    }
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn jsx_and_three_build_as_single_files() {
    for name in ["jsx", "three-game"] {
        require_modules(name);
        let root = example(name);
        let result = build(&root, Target::SingleHtml).await.unwrap_or_else(|e| panic!("{name}: {e:#}"));
        let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
        assert!(html.starts_with("<!doctype html>"), "{name}");
        assert_eq!(result.artifacts.len(), 1);
        std::fs::remove_dir_all(root).ok();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_chat_streams_without_a_model() {
    require_modules("ai-chat");
    let root = example("ai-chat");
    let app = serve(&root).await;
    let res = request(
        &app,
        axum::http::Method::POST,
        "/api/chat",
        &[("content-type", "application/json")],
        r#"{"messages":[{"role":"user","content":"ping"}]}"#,
    )
    .await;
    assert_eq!(res.headers["content-type"], "text/event-stream");
    assert!(res.body.contains(r#"data: {"text":"(local "}"#));
    assert!(res.body.trim_end().ends_with("data: [DONE]"));
    std::fs::remove_dir_all(root).ok();
}
