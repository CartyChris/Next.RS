//! End-to-end tests of the `server` target on dependency-free (vanilla) apps.

mod common;

use axum::http::{Method, StatusCode};
use common::*;

#[tokio::test(flavor = "multi_thread")]
async fn vanilla_example_serves_pages_data_and_apis() {
    let root = example("vanilla");
    let app = serve(&root).await;

    let home = get(&app, "/").await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body.starts_with("<!doctype html>"));
    assert!(home.body.contains("<h1>Hello Forge</h1>"));
    assert!(home.body.contains("<title>Forge Vanilla</title>"));
    let csp = home.headers["content-security-policy"].to_str().unwrap();
    let nonce = csp.split("'nonce-").nth(1).unwrap().split('\'').next().unwrap();
    assert!(nonce.len() >= 16);
    assert_ne!(
        get(&app, "/").await.headers["content-security-policy"],
        home.headers["content-security-policy"],
        "fresh nonce per response"
    );

    let note = get(&app, "/notes/html").await;
    assert!(note.body.contains("<h1>One file</h1>"));
    assert!(note.body.contains("<title>One file · Notes</title>"));
    assert!(note.body.contains(r#""params":{"id":"html"}"#));

    let data = get(&app, "/_forge/data?path=%2Fnotes%2Fgames").await;
    assert_eq!(data.status, StatusCode::OK);
    assert!(data.body.contains(r#""title":"Games""#));
    let etag = data.headers["etag"].to_str().unwrap().to_string();
    let cached = request(&app, Method::GET, "/_forge/data?path=/notes/games", &[("if-none-match", &etag)], "").await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
    assert_eq!(get(&app, "/_forge/data?path=/notes/nope").await.body, r#"{"notFound":true}"#);
    assert_eq!(get(&app, "/_forge/data?path=/about").await.body, r#"{"data":null}"#);

    let missing = get(&app, "/notes/nope").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(missing.body.contains("<h1>404</h1>"), "loader notFound() renders the not-found page");
    assert_eq!(get(&app, "/no/such/page").await.status, StatusCode::NOT_FOUND);

    let api = get(&app, "/api/time?tz=CET").await;
    assert!(api.body.contains(r#""tz":"CET""#));
    let post =
        request(&app, Method::POST, "/api/time", &[("content-type", "application/json")], r#"{"x":[1,2]}"#).await;
    assert_eq!(post.status, StatusCode::CREATED);
    assert_eq!(post.body, r#"{"echo":{"x":[1,2]}}"#);
    let del = request(&app, Method::DELETE, "/api/time", &[], "").await;
    assert_eq!(del.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(del.headers["allow"], "GET, POST");
    assert_eq!(request(&app, Method::POST, "/about", &[], "").await.status, StatusCode::METHOD_NOT_ALLOWED);

    let script =
        home.body.split(r#"<script type="module" src=""#).nth(1).unwrap().split('"').next().unwrap().to_string();
    let js = request(&app, Method::GET, &script, &[("accept-encoding", "gzip, br")], "").await;
    assert_eq!(js.status, StatusCode::OK);
    assert_eq!(js.headers["content-encoding"], "br");
    assert_eq!(js.headers["cache-control"], "public, max-age=31536000, immutable");
    assert_eq!(get(&app, "/_forge/static/../manifest.json").await.status, StatusCode::NOT_FOUND);

    let ready = get(&app, "/health/ready").await;
    assert!(ready.body.contains(r#""ok":true"#));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_requests_are_spread_over_workers() {
    let root = example("vanilla");
    let app = serve(&root).await;
    let tasks: Vec<_> = (0..200)
        .map(|i| {
            let app = app.clone();
            tokio::spawn(async move {
                let path = ["/", "/about", "/notes/rust", "/api/time"][i % 4];
                get(&app, path).await.status
            })
        })
        .collect();
    for t in tasks {
        assert_eq!(t.await.unwrap(), StatusCode::OK);
    }
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn page_cache_revalidate_and_invalidate() {
    let root = project(
        "cache",
        &[
            (
                "app/page.js",
                "export const revalidate = 60;\nlet n = 0;\nexport const render = () => `<p id=n>${++n}</p>`;",
            ),
            (
                "app/api/purge/route.js",
                "import { invalidate } from 'forge:cache';\nexport function POST() { invalidate('/'); return { ok: true }; }",
            ),
            (
                "app/api/kv/route.js",
                "import { cache } from 'forge:cache';\nlet calls = 0;\nexport async function GET() { const v = await cache('k', async () => ++calls, { ttl: 60, tags: ['t'] }); return { v, calls }; }",
            ),
            ("forge.config.json", r#"{"server":{"workers":1}}"#),
        ],
    );
    let app = serve(&root).await;
    let first = get(&app, "/").await;
    assert_eq!(first.headers["x-forge-cache"], "MISS");
    let second = get(&app, "/").await;
    assert_eq!(second.headers["x-forge-cache"], "HIT");
    assert_eq!(first.body, second.body);
    request(&app, Method::POST, "/api/purge", &[], "").await;
    let third = get(&app, "/").await;
    assert_eq!(third.headers["x-forge-cache"], "MISS");
    assert!(third.body.contains("<p id=n>2</p>"));

    assert_eq!(get(&app, "/api/kv").await.body, r#"{"v":1,"calls":1}"#);
    assert_eq!(get(&app, "/api/kv").await.body, r#"{"v":1,"calls":1}"#, "served from forge:cache");
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn client_only_pages_are_not_evaluated_on_the_server() {
    let root = project(
        "csr",
        &[(
            "app/page.js",
            // Touching `document` at module scope would throw if the server evaluated this module.
            "const el = document.createElement('div');\nexport function mount(root) { root.append(el); }",
        )],
    );
    let app = serve(&root).await;
    let res = get(&app, "/").await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(res.body.contains(r#"<div id="__forge_root"></div>"#));
    assert!(res.body.contains(r#""ssr":false"#));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn streams_large_bodies_through_backpressure() {
    let root = project(
        "stream",
        &[
            ("app/page.js", "export const render = () => 'x';"),
            (
                "app/api/big/route.js",
                "export function GET() {\n  const chunk = new TextEncoder().encode('0123456789abcdef'.repeat(4096));\n  let i = 0;\n  return new Response(new ReadableStream({ pull(c) { if (i++ < 160) c.enqueue(chunk); else c.close(); } }));\n}",
            ),
        ],
    );
    let app = serve(&root).await;
    let res = get(&app, "/api/big").await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body.len(), 160 * 16 * 4096);
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn runaway_code_is_stopped_and_the_worker_replaced() {
    let root = project(
        "limits",
        &[
            ("app/page.js", "export const render = () => '<p>alive</p>';"),
            ("app/api/spin/route.js", "export function GET() { for (;;) {} }"),
            (
                "app/api/hog/route.js",
                "export function GET() { const a = []; for (;;) a.push(new Array(1e6).fill(1)); }",
            ),
            ("forge.config.json", r#"{"server":{"workers":1,"cpuMs":200,"memoryLimitMb":64,"timeoutMs":5000}}"#),
        ],
    );
    let app = serve(&root).await;
    let started = std::time::Instant::now();
    let spin = get(&app, "/api/spin").await;
    assert_eq!(spin.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert!(get(&app, "/").await.body.contains("alive"), "replacement worker serves");
    let hog = get(&app, "/api/hog").await;
    assert!(hog.status.is_server_error());
    assert!(get(&app, "/").await.body.contains("alive"), "replacement worker serves after OOM");
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn server_errors_hide_details_in_production() {
    let root = project(
        "errors",
        &[
            ("app/page.js", "export function render() { throw new Error('secret detail'); }"),
            ("app/api/x/route.js", "export function GET() { throw new Error('secret detail'); }"),
        ],
    );
    let app = serve(&root).await;
    for path in ["/", "/api/x"] {
        let res = get(&app, path).await;
        assert_eq!(res.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
        assert!(!res.body.contains("secret detail"), "{path}: {}", res.body);
    }
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn web_platform_apis_in_server_code() {
    let root = project(
        "webapis",
        &[
            ("app/page.js", "export const render = () => '';"),
            (
                "app/api/x/route.js",
                r#"export async function GET() {
  const u = new URL("/a/b?x=1&y=two%20words#h", "https://example.com:8443/base");
  const sp = new URLSearchParams("a=1&a=2&b=%C3%A9");
  const enc = new TextEncoder().encode("héllo");
  const dec = new TextDecoder();
  const split = dec.decode(enc.subarray(0, 2), { stream: true }) + dec.decode(enc.subarray(2));
  const t0 = performance.now();
  await new Promise((r) => setTimeout(r, 20));
  const waited = performance.now() - t0;
  const ac = new AbortController(); ac.abort();
  const h = new Headers({ "X-A": "1" }); h.append("x-a", "2");
  const res = new Response(JSON.stringify({ ok: 1 }), { headers: { "content-type": "application/json" } });
  return {
    href: u.href, host: u.host, path: u.pathname, y: u.searchParams.get("y"), origin: u.origin,
    all: sp.getAll("a"), b: sp.get("b"), bytes: enc.length, split,
    waited: waited >= 15, aborted: ac.signal.aborted, h: h.get("x-a"),
    json: await res.json(), uuid: crypto.randomUUID().length, b64: atob(btoa("hi!")),
  };
}"#,
            ),
        ],
    );
    let app = serve(&root).await;
    let res = get(&app, "/api/x").await;
    let v: serde_json::Value = serde_json::from_str(&res.body).unwrap_or_else(|_| panic!("{}", res.body));
    assert_eq!(v["href"], "https://example.com:8443/a/b?x=1&y=two%20words#h");
    assert_eq!(v["host"], "example.com:8443");
    assert_eq!(v["path"], "/a/b");
    assert_eq!(v["y"], "two words");
    assert_eq!(v["origin"], "https://example.com:8443");
    assert_eq!(v["all"], serde_json::json!(["1", "2"]));
    assert_eq!(v["b"], "é");
    assert_eq!(v["bytes"], 6);
    assert_eq!(v["split"], "héllo");
    assert_eq!(v["waited"], true);
    assert_eq!(v["aborted"], true);
    assert_eq!(v["h"], "1, 2");
    assert_eq!(v["json"]["ok"], 1);
    assert_eq!(v["uuid"], 36);
    assert_eq!(v["b64"], "hi!");
    std::fs::remove_dir_all(root).ok();
}
