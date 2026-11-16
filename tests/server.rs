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

#[tokio::test(flavor = "multi_thread")]
async fn review_regressions_server() {
    let root = project(
        "review",
        &[
            ("app/page.js", "export const render = ({ search }) => `<p id=q>${search}</p>`;"),
            (
                "app/loader.js",
                "import logo from './logo.svg';\nexport const loader = ({ url }) => ({ scheme: url.protocol, logo });",
            ),
            ("app/logo.svg", "<svg xmlns='http://www.w3.org/2000/svg'/>"),
            ("public/ok.txt", "public file"),
        ],
    );
    let outside = root.with_extension("secret");
    std::fs::write(&outside, "do not serve").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("public/leak.txt")).unwrap();
    let app = serve(&root).await;

    // The query string reaches server rendering.
    assert!(get(&app, "/?q=rust").await.body.contains("<p id=q>?q=rust</p>"));
    // Loader URLs keep the forwarded scheme on the data endpoint too.
    let data = request(&app, Method::GET, "/_forge/data?path=/", &[("x-forwarded-proto", "https")], "").await;
    assert!(data.body.contains(r#""scheme":"https:""#), "{}", data.body);
    // An asset imported only by server code is still served.
    let v: serde_json::Value = serde_json::from_str(&data.body).unwrap();
    let logo = v["data"]["logo"].as_str().unwrap().to_string();
    assert_eq!(get(&app, &logo).await.status, StatusCode::OK, "{logo}");
    // public/ symlinks cannot reach outside the directory.
    assert_eq!(get(&app, "/ok.txt").await.status, StatusCode::OK);
    #[cfg(unix)]
    assert_eq!(get(&app, "/leak.txt").await.status, StatusCode::NOT_FOUND);
    std::fs::remove_dir_all(root).ok();
    std::fs::remove_file(outside).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn cache_misses_compute_once_across_workers() {
    let root = project(
        "single-flight",
        &[
            ("app/page.js", "export const render = () => '';"),
            (
                "app/api/value/route.js",
                r#"import { cache, set, keys } from "forge:cache";
export async function GET() {
  const v = await cache("shared", async () => {
    set("ran:" + Math.random(), 1);
    await new Promise((r) => setTimeout(r, 300));
    return "value";
  });
  return { v, runs: keys("ran:").length };
}"#,
            ),
            ("forge.config.json", r#"{"server":{"workers":4}}"#),
        ],
    );
    let app = serve(&root).await;
    // Concurrent misses land on different workers (least-loaded dispatch).
    let all = futures_util::future::join_all((0..8).map(|_| get(&app, "/api/value"))).await;
    for r in &all {
        assert!(r.body.starts_with(r#"{"v":"value","#), "{}", r.body);
    }
    assert_eq!(get(&app, "/api/value").await.body, r#"{"v":"value","runs":1}"#, "computed more than once");
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn text_encoder_encode_into_writes_whole_characters() {
    let root = project(
        "encode-into",
        &[
            ("app/page.js", "export const render = () => '';"),
            (
                "app/api/enc/route.js",
                r#"export function GET() {
  const enc = new TextEncoder();
  const run = (s, n) => { const d = new Uint8Array(n); const r = enc.encodeInto(s, d); return [r.read, r.written, Array.from(d.subarray(0, r.written))]; };
  return [run("é", 1), run("aé", 2), run("aé", 3), run("😀x", 4), run("😀x", 3), run("a\uD800b", 10)];
}"#,
            ),
        ],
    );
    let app = serve(&root).await;
    assert_eq!(
        get(&app, "/api/enc").await.body,
        "[[0,0,[]],[1,1,[97]],[2,3,[97,195,169]],[2,4,[240,159,152,128]],[0,0,[]],[3,5,[97,239,191,189,98]]]"
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn server_intl_matches_browser_en_us() {
    // The engine has no ICU; these are the strings V8 prints for en-US.
    let root = project(
        "intl",
        &[
            ("app/page.js", "export const render = () => '';"),
            (
                "app/api/fmt/route.js",
                r#"export function GET() {
  const d = new Date(Date.UTC(2026, 10, 16, 15, 4, 5));
  const utc = { timeZone: "UTC" };
  return [
    (1234567.891).toLocaleString(), (18240).toLocaleString("en-US"), (0.5).toLocaleString(),
    new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(1234.5),
    new Intl.NumberFormat("en-US", { style: "currency", currency: "JPY" }).format(1234.5),
    new Intl.NumberFormat("en-US", { style: "percent" }).format(0.256),
    new Intl.NumberFormat("en-US", { style: "percent", maximumFractionDigits: 1 }).format(0.2567),
    new Intl.NumberFormat("en-US", { notation: "compact" }).format(1234),
    new Intl.NumberFormat("en-US", { notation: "compact" }).format(15300000),
    new Intl.NumberFormat("en-US", { minimumFractionDigits: 2 }).format(3),
    new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(-2.5e3),
    new Intl.NumberFormat("en-US", { signDisplay: "always" }).format(12),
    d.toLocaleDateString("en-US", utc),
    d.toLocaleDateString("en-US", { ...utc, month: "short", day: "numeric", year: "numeric" }),
    d.toLocaleDateString("en-US", { ...utc, weekday: "long", month: "long", day: "numeric" }),
    d.toLocaleTimeString("en-US", utc),
    d.toLocaleTimeString("en-US", { ...utc, hour: "numeric", minute: "2-digit" }),
    d.toLocaleString("en-US", utc),
    new Intl.DateTimeFormat("en-US", { ...utc, dateStyle: "medium" }).format(d),
    new Intl.DateTimeFormat("en-US", { ...utc, month: "long", year: "numeric" }).format(d),
    new Intl.PluralRules("en-US").select(1), new Intl.PluralRules("en-US", { type: "ordinal" }).select(22),
    new Intl.RelativeTimeFormat("en", { numeric: "auto" }).format(-1, "day"),
    new Intl.RelativeTimeFormat("en").format(3, "hour"),
    new Intl.ListFormat("en").format(["a", "b", "c"]),
    new Intl.NumberFormat("en-US", { notation: "compact" }).format(999999),
    new Intl.NumberFormat("en-US", { notation: "compact" }).format(123456),
    new Intl.NumberFormat("en-US", { style: "unit", unit: "kilobyte" }).format(12),
  ];
}"#,
            ),
        ],
    );
    let app = serve(&root).await;
    let got: Vec<String> = serde_json::from_str(&get(&app, "/api/fmt").await.body).unwrap();
    let want = [
        "1,234,567.891",
        "18,240",
        "0.5",
        "$1,234.50",
        "¥1,235",
        "26%",
        "25.7%",
        "1.2K",
        "15M",
        "3.00",
        "-2,500",
        "+12",
        "11/16/2026",
        "Nov 16, 2026",
        "Monday, November 16",
        "3:04:05 PM",
        "3:04 PM",
        "11/16/2026, 3:04:05 PM",
        "Nov 16, 2026",
        "November 2026",
        "one",
        "two",
        "yesterday",
        "in 3 hours",
        "a, b, and c",
        "1M",
        "123K",
        "12 kB",
    ];
    assert_eq!(got, want);
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn n8n_webhooks_in_both_directions() {
    use axum::{extract::Request as AxumRequest, routing::any};
    // A stand-in for n8n: a Webhook node that echoes what it received, and one that is inactive.
    let n8n = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let n8n_url = format!("http://{}", n8n.local_addr().unwrap());
    let router = axum::Router::new()
        .route(
            "/webhook/order",
            any(|req: AxumRequest| async move {
                let secret = req.headers().get("x-n8n-secret").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                let query = req.uri().query().unwrap_or("").to_string();
                let body = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap();
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
                if secret != "s3cret" {
                    return (StatusCode::FORBIDDEN, "{\"message\":\"Authorization data is wrong!\"}".to_string());
                }
                (
                    StatusCode::OK,
                    serde_json::json!({ "received": body, "query": query, "workflow": "order" }).to_string(),
                )
            }),
        )
        .route("/webhook/missing", any(|| async { (StatusCode::NOT_FOUND, "{\"message\":\"not registered\"}") }));
    tokio::spawn(async move { axum::serve(n8n, router).await });

    let root = project(
        "n8n",
        &[
            ("app/page.js", "export const render = () => '';"),
            (
                "app/api/order/route.js",
                r#"import { trigger, WorkflowError } from "forge:n8n";
export async function POST(request, { env, searchParams }) {
  try {
    const path = searchParams.get("path") ?? "order";
    const result = await trigger(`${env.N8N_URL}/webhook/${path}`, await request.json(), {
      secret: searchParams.get("secret") ?? env.N8N_SECRET, query: { source: "forge" },
    });
    return { ok: true, result };
  } catch (e) {
    return Response.json({ ok: false, name: e.name, isWorkflowError: e instanceof WorkflowError, status: e.status, message: e.message }, { status: 502 });
  }
}"#,
            ),
            (
                "app/api/hook/route.js",
                r#"import { webhook, WorkflowError } from "forge:n8n";
export const POST = webhook(async ({ body, query }) => {
  if (!body?.id) throw new WorkflowError("id is required", 422);
  return { stored: body.id, tag: query.tag ?? null };
}, { secret: (ctx) => ctx.env.N8N_SECRET });"#,
            ),
            ("forge.config.json", r#"{"server":{"allowFetch":["127.0.0.1"],"env":["N8N_URL","N8N_SECRET"]}}"#),
            (".env", &format!("N8N_URL={n8n_url}\nN8N_SECRET=s3cret\n")),
        ],
    );
    let app = serve(&root).await;
    let json =
        |r: &Res| serde_json::from_str::<serde_json::Value>(&r.body).unwrap_or_else(|_| panic!("not JSON: {}", r.body));

    // App → n8n: the secret header, the JSON body and the query all arrive; the reply is parsed.
    let sent =
        request(&app, Method::POST, "/api/order", &[("content-type", "application/json")], r#"{"sku":"A1","qty":2}"#)
            .await;
    let v = json(&sent);
    assert_eq!(v["result"]["received"], serde_json::json!({ "sku": "A1", "qty": 2 }), "{v}");
    assert_eq!(v["result"]["query"], "source=forge");
    // Failures carry the status and a hint about the likely cause.
    let wrong = json(&request(&app, Method::POST, "/api/order?secret=nope", &[], "{}").await);
    assert_eq!((wrong["isWorkflowError"].as_bool(), wrong["status"].as_u64()), (Some(true), Some(403)), "{wrong}");
    assert!(wrong["message"].as_str().unwrap().contains("authentication"), "{wrong}");
    let inactive = json(&request(&app, Method::POST, "/api/order?path=missing", &[], "{}").await);
    assert!(inactive["message"].as_str().unwrap().contains("is the workflow active"), "{inactive}");

    // n8n → app: secret enforced (header or bearer), JSON and form bodies parsed, errors mapped.
    let none = request(&app, Method::POST, "/api/hook", &[], r#"{"id":7}"#).await;
    assert_eq!(none.status, 401);
    assert!(none.body.contains("x-n8n-secret"), "{}", none.body);
    let bad = request(&app, Method::POST, "/api/hook", &[("x-n8n-secret", "s3creT")], r#"{"id":7}"#).await;
    assert_eq!(bad.status, 401);
    let ok = request(
        &app,
        Method::POST,
        "/api/hook?tag=vip",
        &[("x-n8n-secret", "s3cret"), ("content-type", "application/json")],
        r#"{"id":7}"#,
    )
    .await;
    assert_eq!((ok.status.as_u16(), ok.body.as_str()), (200, r#"{"stored":7,"tag":"vip"}"#));
    let bearer = request(
        &app,
        Method::POST,
        "/api/hook",
        &[("authorization", "Bearer s3cret"), ("content-type", "application/x-www-form-urlencoded")],
        "id=form-1",
    )
    .await;
    assert_eq!(json(&bearer)["stored"], "form-1");
    let invalid = request(
        &app,
        Method::POST,
        "/api/hook",
        &[("x-n8n-secret", "s3cret"), ("content-type", "application/json")],
        r#"{"other":1}"#,
    )
    .await;
    assert_eq!(
        (invalid.status.as_u16(), json(&invalid)["error"].as_str().unwrap().to_string()),
        (422, "id is required".to_string())
    );
    std::fs::remove_dir_all(root).ok();
}
