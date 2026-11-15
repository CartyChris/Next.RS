//! Live single-file development: `forge dev --target single-html` and `forge pack --watch`.

mod common;

use std::time::{Duration, Instant};

use common::project;

async fn get(base: &str) -> (u16, String) {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let res = client.get(format!("{base}/")).send().await.unwrap();
    (res.status().as_u16(), res.text().await.unwrap())
}

/// Poll `/` until `pred` holds (rebuilds are asynchronous).
async fn until(base: &str, what: &str, pred: impl Fn(u16, &str) -> bool) -> (u16, String) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (status, body) = get(base).await;
        if pred(status, &body) {
            return (status, body);
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}; last response {status}:\n{body}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The first `reload` event id after the `hello`.
async fn next_reload(base: &str) -> tokio::task::JoinHandle<String> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut res = client.get(format!("{base}/_forge/live")).send().await.unwrap();
    tokio::spawn(async move {
        let mut buf = String::new();
        while let Some(chunk) = res.chunk().await.unwrap() {
            buf.push_str(&String::from_utf8_lossy(&chunk));
            if let Some(i) = buf.find("event: reload\ndata: ") {
                return buf[i + 20..].lines().next().unwrap_or("").to_string();
            }
        }
        String::new()
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn single_html_dev_serves_the_real_file_and_reloads() {
    let root = project(
        "single-dev",
        &[("app/page.js", "export const render = () => '<h1>first</h1>';\nexport function mount() {}")],
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(nextrs::server::single::run_project_on(root.clone(), listener));

    let (_, page) = until(&base, "the first build", |s, b| s == 200 && b.contains("<h1>first</h1>")).await;
    // The single file itself: inline module, hash routing, plus only the live-reload tag.
    assert!(page.contains(r#"id="__forge_app""#) && !page.contains("/_forge/static/"), "{page}");
    assert!(page.contains(r#"<script type="module" src="/_forge/live.js"></script></body>"#), "{page}");
    assert!(!page.contains("DecompressionStream"), "dev builds stay readable:\n{page}");
    assert_eq!(get(&format!("{base}/about")).await.0, 404, "single files route with #/…");

    let reload = next_reload(&base).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    std::fs::write(
        root.join("app/page.js"),
        "export const render = () => '<h1>second</h1>';\nexport function mount() {}",
    )
    .unwrap();
    until(&base, "the edit", |s, b| s == 200 && b.contains("<h1>second</h1>")).await;
    let id = tokio::time::timeout(Duration::from_secs(10), reload).await.expect("a reload event").unwrap();
    assert!(!id.is_empty());

    // A broken file shows the error in the page, with live reload to recover.
    std::fs::write(root.join("app/page.js"), "export const render = () => '<h1>broken</h1>;").unwrap();
    let (_, err) = until(&base, "the error page", |s, _| s == 500).await;
    assert!(err.contains("Build failed") && err.contains("/_forge/live.js"), "{err}");
    std::fs::write(root.join("app/page.js"), "export const render = () => '<h1>third</h1>';").unwrap();
    until(&base, "recovery", |s, b| s == 200 && b.contains("<h1>third</h1>")).await;
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn pack_watch_repacks_on_change_without_looping() {
    let root = project(
        "pack-watch",
        &[
            (
                "index.html",
                "<html><head><link rel=\"stylesheet\" href=\"style.css\"></head><body><p>hi</p></body></html>",
            ),
            ("style.css", "p { color: red }"),
        ],
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let options = nextrs::pack::Options {
        cache_dir: root.join(".cache"),
        registry: "http://127.0.0.1:9".into(),
        ..Default::default()
    };
    tokio::spawn(nextrs::pack::watch_on(root.join("index.html"), None, options, listener));

    until(&base, "the first pack", |s, b| s == 200 && b.contains("p { color: red }")).await;
    assert!(root.join("index.packed.html").is_file());
    std::fs::write(root.join("style.css"), "p { color: blue }").unwrap();
    until(&base, "the CSS edit", |s, b| s == 200 && b.contains("p { color: blue }")).await;

    // Writing the output must not trigger more packs: the file settles.
    tokio::time::sleep(Duration::from_millis(600)).await;
    let settled = std::fs::metadata(root.join("index.packed.html")).unwrap().modified().unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        std::fs::metadata(root.join("index.packed.html")).unwrap().modified().unwrap(),
        settled,
        "repacked with no change"
    );
    std::fs::remove_dir_all(root).ok();
}
