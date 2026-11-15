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
    let packed = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(packed.contains("DecompressionStream") && packed.contains("<h1>Hello Forge</h1>"), "compressed by default");
    // Uncompressed, the embedded loader data is visible in the file.
    std::fs::write(root.join("forge.config.json"), r#"{"title":"Forge React","build":{"compress":false}}"#).unwrap();
    build(&root, Target::SingleHtml).await.unwrap();
    let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(html.len() > packed.len() * 2 && !html.contains("DecompressionStream"));
    assert!(html.contains("<h1>Hello Forge</h1>"));
    assert!(html.contains(r#"Single-file"#) || html.contains("single-file"), "blog loader data embedded");
    assert!(html.contains(r##"<a href="#/blog">"##), "prerendered Links use hash routing");
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

#[tokio::test(flavor = "multi_thread")]
async fn arena_rooms_lobby_and_offline_build() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    require_modules("arena");
    let root = example("arena");
    let base = listen(&root).await;
    let lobby = reqwest::get(format!("{base}/")).await.unwrap().text().await.unwrap();
    assert!(lobby.contains("Collect the light."), "lobby is server-rendered");

    let (mut ws, _) = connect_async(format!("{}/api/room/ci", base.replace("http", "ws"))).await.unwrap();
    ws.send(Message::text(r#"{"t":"join","name":"Robot Tester"}"#)).await.unwrap();
    let mut seen = std::collections::HashSet::new();
    let mut me_in_snapshot = false;
    while !(seen.contains("hi") && seen.contains("roster") && me_in_snapshot) {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .expect("room is alive")
            .unwrap()
            .unwrap();
        let Message::Text(t) = msg else { continue };
        let v: serde_json::Value = serde_json::from_str(&t).unwrap();
        let kind = v["t"].as_str().unwrap_or_default().to_string();
        if kind == "roster" && v["players"].as_array().unwrap().iter().any(|p| p["name"] == "Robot Tester") {
            seen.insert("roster".to_string());
        } else if kind == "s" && seen.contains("roster") {
            me_in_snapshot = v["p"].as_array().unwrap().len() == 4; // three bots + us
        } else {
            seen.insert(kind);
        }
    }
    let status: serde_json::Value = reqwest::get(format!("{base}/api/room/ci")).await.unwrap().json().await.unwrap();
    assert_eq!((status["humans"].as_i64(), status["bots"].as_i64()), (Some(1), Some(3)), "{status}");

    let bad = reqwest::Client::new()
        .post(format!("{base}/api/champion"))
        .body(r#"{"weights":[1,2,3]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400, "malformed brains are rejected");

    let result = build(&root, Target::SingleHtml).await.unwrap();
    let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(html.len() < 400_000, "offline arena is {} bytes", html.len());
    assert_eq!(result.artifacts.len(), 1);
    std::fs::remove_dir_all(root).ok();
}
