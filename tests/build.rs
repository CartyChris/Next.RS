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
