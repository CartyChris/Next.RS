//! Build targets and the security policy.

mod common;

use common::*;
use nextrs::config::Target;

#[tokio::test(flavor = "multi_thread")]
async fn server_build_writes_the_expected_layout() {
    let root = example("vanilla");
    let result = build(&root, Target::Server).await.unwrap();
    for f in [
        ".forge/generated/server-entry.js",
        ".forge/generated/client-entry.js",
        ".forge/generated/routes/route_home.client.js",
        ".forge/server/server.js",
        ".forge/manifest.json",
        ".forge/capsule.json",
        ".forge/bundle-stats.json",
        ".forge/types/forge.d.ts",
    ] {
        assert!(root.join(f).is_file(), "missing {f}");
    }
    let m = &result.manifest;
    assert!(m.capabilities.ssr && !m.capabilities.single_file);
    assert_eq!(
        m.routes.iter().map(|r| r.pattern.as_str()).collect::<Vec<_>>(),
        ["/api/time", "/notes/[id]", "/about", "/"]
    );
    assert!(m.client.entry.starts_with("/_forge/static/client."));
    assert!(m.client.preload["route_notes_id"].iter().any(|u| u.contains("route_notes_id")));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn static_build_prerenders_every_page() {
    let root = example("vanilla");
    let result = build(&root, Target::Static).await.unwrap();
    let dist = root.join("dist");
    let read = |p: &str| std::fs::read_to_string(dist.join(p)).unwrap_or_else(|_| panic!("missing dist/{p}"));
    assert!(read("index.html").contains("<h1>Hello Forge</h1>"));
    assert!(read("about/index.html").contains("<h1>About</h1>"));
    for id in ["rust", "html", "games"] {
        assert!(read(&format!("notes/{id}/index.html")).contains(&format!(r#""params":{{"id":"{id}"}}"#)));
        assert!(read(&format!("_forge/data/notes/{id}.json")).starts_with(r#"{"data":"#));
    }
    assert!(read("404.html").contains("<h1>404</h1>"));
    assert!(!result.manifest.capabilities.ssr && result.manifest.capabilities.prerendered);
    assert!(result.warnings.iter().any(|w| w.contains("API routes")));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn single_html_build_is_one_self_contained_file() {
    let root = example("vanilla");
    let result = build(&root, Target::SingleHtml).await.unwrap();
    let files: Vec<_> = std::fs::read_dir(root.join("dist")).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(html.contains("<h1>Hello Forge</h1>"), "home is prerendered");
    assert!(html.contains("<style>"), "css inlined");
    assert!(!html.contains(" src=") && !html.contains("<link rel=\"stylesheet\""), "no external resources");
    assert!(html.contains(r#""rust":{"id":"rust""#) || html.contains(r#""/notes/rust":{"#), "loader data embedded");
    assert!(result.manifest.capabilities.single_file && !result.manifest.capabilities.server_loaders);
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn single_html_inlines_assets_and_escapes_script_terminators() {
    let root = project(
        "inline",
        &[
            ("app/logo.svg", "<svg xmlns='http://www.w3.org/2000/svg'/>"),
            ("app/shader.frag", "void main() { gl_FragColor = vec4(1.0); }"),
            (
                "app/page.js",
                "import logo from './logo.svg';\nimport frag from './shader.frag';\nexport const s = '</script><script>alert(1)</script>';\nexport function mount(el) { el.innerHTML = `<img src=${logo}>` + frag + s; }",
            ),
            ("forge.config.json", r#"{"build":{"minify":false}}"#),
        ],
    );
    build(&root, Target::SingleHtml).await.unwrap();
    let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(html.contains("data:image/svg+xml;base64,"));
    assert!(html.contains("gl_FragColor"));
    assert_eq!(html.matches("</script>").count(), 2, "only the boot data and module script close");
    assert!(html.contains(r"<\/script><script>alert(1)<\/script>"));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn security_policy_is_enforced_at_build_time() {
    let cases: &[(&str, &str, &str)] = &[
        ("app/page.js", "export const render = () => String(eval('1+1'));", "`eval` is denied"),
        ("app/page.js", "export const render = () => String(new Function('return 1')());", "`new Function`"),
        (
            "app/page.js",
            "import x from 'ext:forge_runtime/ops.js';\nexport const render = () => x;",
            "internal to Forge",
        ),
        (
            "app/page.js",
            "import x from 'https://cdn.example.com/x.js';\nexport const render = () => x;",
            "remote import",
        ),
        ("app/page.js", "import x from 'forge:nope';\nexport const render = () => x;", "unknown module `forge:nope`"),
        ("app/page.js", "export const x = 1;", "a page must default-export"),
        (
            "app/api/x/route.js",
            "import { readFileSync } from 'fs';\nexport const GET = () => readFileSync('/x');",
            "Node.js built-in",
        ),
        ("app/page.js", "import x from 'not-installed-pkg';\nexport const render = () => x;", "unresolved imports"),
        ("app/api/x/route.js", "export const handler = () => 1;", "must export at least one of GET"),
    ];
    for (file, source, expected) in cases {
        let root = project("policy", &[(file, source), ("app/ok/page.js", "export const render = () => '';")]);
        let err = build(&root, Target::Server).await.map(|_| ()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains(expected), "{file}: expected `{expected}` in:\n{msg}");
        std::fs::remove_dir_all(root).ok();
    }
    // Shared helpers are scanned too, with positions.
    let root = project(
        "helper",
        &[
            ("app/page.js", "import { f } from './lib/x.js';\nexport const render = f;"),
            ("app/lib/x.js", "export const f = () =>\n  eval('1');"),
        ],
    );
    let msg = format!("{:#}", build(&root, Target::Server).await.map(|_| ()).unwrap_err());
    assert!(msg.contains("app/lib/x.js:2:3"), "{msg}");
    std::fs::remove_dir_all(root).ok();
    // allowEval opts out of the eval rule only.
    let root = project(
        "alloweval",
        &[
            ("app/page.js", "export const render = () => String(eval('1+1'));"),
            ("forge.config.json", r#"{"security":{"allowEval":true}}"#),
        ],
    );
    build(&root, Target::Server).await.unwrap();
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn wasm_host_is_reported_as_not_implemented() {
    let root = example("vanilla");
    let err = build(&root, Target::WasmHost).await.unwrap_err();
    assert!(err.to_string().contains("planned"));
    std::fs::remove_dir_all(root).ok();
}

const WORKER_PAGE: &str = r#"
export function mount(el) {
  const worker = new Worker(new URL("./lib/primes.worker.js", import.meta.url), { type: "module" });
  worker.onmessage = async (e) => {
    const bin = await (await fetch(new URL("./lib/table.bin", import.meta.url))).arrayBuffer();
    el.innerHTML = `<p id="out">${e.data.count} primes, ${bin.byteLength} bytes</p>`;
  };
  worker.postMessage(1000);
  return () => worker.terminate();
}
"#;

const WORKER: &str = r#"
import { isPrime } from "./math.js";
const words = new URL("./words.txt", import.meta.url);
self.onmessage = (e) => {
  let count = 0;
  for (let i = 2; i < e.data; i++) if (isPrime(i)) count++;
  self.postMessage({ count, words: words.href.slice(0, 5) });
};
"#;

fn worker_project(name: &str) -> std::path::PathBuf {
    project(
        name,
        &[
            ("app/page.js", WORKER_PAGE),
            ("app/lib/primes.worker.js", WORKER),
            (
                "app/lib/math.js",
                "export const isPrime = (n) => { for (let d = 2; d * d <= n; d++) if (n % d === 0) return false; return true; };",
            ),
            ("app/lib/words.txt", "hello"),
            ("app/lib/table.bin", "0123456789"),
        ],
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn workers_and_asset_urls_are_bundled() {
    let root = worker_project("workers");
    build(&root, Target::Server).await.unwrap();
    let client = root.join(".forge/client");
    let workers: Vec<_> = std::fs::read_dir(client.join("workers"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    assert_eq!(workers.len(), 1, "{workers:?}");
    assert!(workers[0].starts_with("primes_worker.") && workers[0].ends_with(".js"));
    let worker_code = std::fs::read_to_string(client.join("workers").join(&workers[0])).unwrap();
    assert!(worker_code.contains("isPrime") || worker_code.contains("postMessage"), "helper bundled into the worker");
    assert!(!worker_code.contains("import "), "worker is self-contained");
    let assets: Vec<_> = std::fs::read_dir(client.join("assets"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    assert!(assets.iter().any(|a| a.starts_with("table.") && a.ends_with(".bin")), "{assets:?}");
    assert!(assets.iter().any(|a| a.starts_with("words.") && a.ends_with(".txt")), "worker's own asset: {assets:?}");
    let chunks = std::fs::read_dir(client.join("chunks"))
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect::<String>();
    assert!(chunks.contains(&format!("/_forge/static/workers/{}", workers[0])));

    build(&root, Target::SingleHtml).await.unwrap();
    let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
    assert!(html.contains("__forgeWorkerUrl"), "worker inlined as a blob");
    assert!(html.contains("data:application/octet-stream;base64,MDEyMzQ1Njc4OQ=="), "asset inlined");
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn review_regressions_build() {
    // A staticParams() entry shadowed by a static page must not overwrite it.
    let root = project(
        "shadow",
        &[
            ("app/page.js", "export const render = () => 'home';"),
            ("app/blog/archive/page.js", "export const render = () => '<h1>Archive page</h1>';"),
            ("app/blog/[slug]/page.js", "export const render = ({ data }) => `<h1>Post ${data.slug}</h1>`;"),
            (
                "app/blog/[slug]/loader.js",
                "export const loader = ({ params }) => ({ slug: params.slug });\nexport const staticParams = () => [{ slug: 'archive' }, { slug: 'one' }];",
            ),
        ],
    );
    build(&root, Target::Static).await.unwrap();
    let archive = std::fs::read_to_string(root.join("dist/blog/archive/index.html")).unwrap();
    assert!(archive.contains("<h1>Archive page</h1>"), "{archive}");
    assert!(std::fs::read_to_string(root.join("dist/blog/one/index.html")).unwrap().contains("<h1>Post one</h1>"));
    assert!(!root.join("dist/_forge/data/blog/archive.json").exists());
    std::fs::remove_dir_all(root).ok();

    // Client-only pages are scanned even though the server bundle never imports them.
    let root =
        project("csr-eval", &[("app/page.js", "export const ssr = false;\nexport function mount() { eval('1'); }")]);
    let err = build(&root, Target::Server).await.map(|_| ()).unwrap_err();
    assert!(format!("{err:#}").contains("`eval` is denied"), "{err:#}");
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn forge_file_and_target_constant() {
    // `import.meta.env.TARGET` is a build-time constant, and forge:file imports cleanly on the
    // server (prerendering) as well as in the browser bundle.
    let page = r#"import { embeddedState, fileName } from "forge:file";
export const render = () => `<p id="t">${import.meta.env.TARGET}|${embeddedState() === undefined}|${fileName()}</p>`;
export function mount(el) { el.dataset.target = import.meta.env.TARGET; }"#;
    for (target, expect) in [(Target::SingleHtml, "single-html"), (Target::Static, "static")] {
        let root = project(&format!("file-{expect}"), &[("app/page.js", page)]);
        build(&root, target).await.unwrap();
        let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
        assert!(html.contains(&format!(r#"<p id="t">{expect}|true|index.html</p>"#)), "{html}");
        // Only single files carry the id that saveSelf() copies the app script by.
        assert_eq!(html.contains(r#"id="__forge_app""#), target == Target::SingleHtml, "{html}");
        std::fs::remove_dir_all(root).ok();
    }
}
