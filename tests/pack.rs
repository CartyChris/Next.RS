//! `forge pack` end to end, hermetic: a fake npm registry and a fake web server on localhost.
#![cfg(feature = "pack")]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{Router, extract::Path as UrlPath, response::IntoResponse};
use base64::Engine as _;
use nextrs::{config::Compress, pack};
use sha2::Digest as _;

type Files = Vec<(&'static str, Vec<u8>)>;

fn tarball(files: &Files) -> Vec<u8> {
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for (path, body) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, format!("package/{path}"), body.as_slice()).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn b64(d: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(d)
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-png-bytes";

/// Packages: name → (version, files).
fn packages() -> HashMap<&'static str, (&'static str, Files)> {
    let mut m = HashMap::new();
    m.insert(
        "mini3d",
        (
            "1.0.0",
            vec![
                ("package.json", br#"{"name":"mini3d","version":"1.0.0","module":"build/mini3d.module.js","main":"build/mini3d.umd.js"}"#.to_vec()),
                ("build/mini3d.module.js", b"export const VERSION = 'mini3d-1';\nexport function scene() { return 'scene'; }\n".to_vec()),
                ("examples/jsm/orbit.js", b"import { VERSION } from 'mini3d';\nexport const orbit = 'orbit+' + VERSION;\n".to_vec()),
                ("build/mini3d.umd.js", b"window.Mini3D = { umd: 'mini3d-umd' };\n".to_vec()),
                ("css/theme.css", b".theme { background: url(../img/dot.png) }\n".to_vec()),
                ("img/dot.png", PNG.to_vec()),
            ],
        ),
    );
    m
}

/// A registry at `/` (metadata and tarballs) and a plain web server under `/web/`.
async fn servers() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let pkgs = Arc::new(packages());
    let (p1, p2, b) = (pkgs.clone(), pkgs.clone(), base.clone());
    let app = Router::new()
        .route(
            "/{name}",
            axum::routing::get(move |UrlPath(name): UrlPath<String>| {
                let (pkgs, base) = (p1.clone(), b.clone());
                async move {
                    let Some((version, files)) = pkgs.get(name.as_str()) else {
                        return (axum::http::StatusCode::NOT_FOUND, String::new()).into_response();
                    };
                    let tgz = tarball(files);
                    let integrity = format!("sha512-{}", b64(&sha2::Sha512::digest(&tgz)));
                    serde_json::json!({
                        "name": name,
                        "dist-tags": { "latest": version },
                        "versions": { *version: { "name": name, "version": version,
                            "dist": { "tarball": format!("{base}/-/{name}-{version}.tgz"), "integrity": integrity } } }
                    })
                    .to_string()
                    .into_response()
                }
            }),
        )
        .route(
            "/-/{file}",
            axum::routing::get(move |UrlPath(file): UrlPath<String>| {
                let pkgs = p2.clone();
                async move {
                    let name = file.rsplit_once('-').unwrap().0;
                    tarball(&pkgs[name].1).into_response()
                }
            }),
        )
        .route(
            "/web/{*path}",
            axum::routing::get(|UrlPath(path): UrlPath<String>| async move {
                match path.as_str() {
                    "lib.js" => ([("content-type", "text/javascript")], b"window.remoteLib = 'remote-lib';".to_vec())
                        .into_response(),
                    "pic.png" => ([("content-type", "image/png")], PNG.to_vec()).into_response(),
                    "fonts.css" => (
                        [("content-type", "text/css")],
                        b"@font-face { font-family: X; src: url(/web/f.woff2) format('woff2'); }".to_vec(),
                    )
                        .into_response(),
                    "f.woff2" => ([("content-type", "font/woff2")], b"wOF2fake-font".to_vec()).into_response(),
                    _ => axum::http::StatusCode::NOT_FOUND.into_response(),
                }
            }),
        );
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, handle)
}

fn site(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forge-pack-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, body) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    dir
}

fn options(registry: &str, cache: &Path) -> pack::Options {
    pack::Options {
        compress: Compress::Auto,
        minify: false,
        offline: false,
        registry: registry.into(),
        cache_dir: cache.to_path_buf(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn packs_a_cdn_page_into_one_offline_file() {
    // reqwest honours HTTP(S)_PROXY; the fake servers are local.
    unsafe { std::env::set_var("NO_PROXY", "127.0.0.1,localhost") };
    unsafe { std::env::set_var("no_proxy", "127.0.0.1,localhost") };
    let (base, server) = servers().await;
    let umd = b"window.Mini3D = { umd: 'mini3d-umd' };\n";
    let sri = format!("sha384-{}", b64(&sha2::Sha384::digest(umd)));
    let page = format!(
        r#"<!doctype html><html><head>
<meta http-equiv="Content-Security-Policy" content="default-src 'none'">
<link rel="preload" href="second.js" as="script">
<link rel="stylesheet" href="https://unpkg.com/mini3d@1.0.0/css/theme.css">
<link rel="stylesheet" href="{base}/web/fonts.css">
<link rel="stylesheet" href="local.css" media="print" onload="this.media='all'">
<script type="importmap">{{"imports":{{"mini3d":"https://unpkg.com/mini3d@1.0.0/build/mini3d.module.js","mini3d/addons/":"https://cdn.jsdelivr.net/npm/mini3d@1.0.0/examples/jsm/"}}}}</script>
<script src="https://unpkg.com/mini3d@1.0.0/build/mini3d.umd.js" integrity="{sri}"></script>
<script src="{base}/web/lib.js" defer></script>
<script nomodule src="legacy.js"></script>
</head><body>
<img src="pic.png" srcset="pic.png 1x, pic2x.png 2x" alt="local">
<img src="{base}/web/pic.png" alt="remote">
<img src=".secret/key.png" alt="hidden"><img src="../outside.png" alt="outside">
<div id="bg" style="background:url('pic.png')"></div>
<script type="module">import {{ VERSION }} from "mini3d"; import {{ orbit }} from "mini3d/addons/orbit.js"; import {{ app, img }} from "./app.js"; window.result = [VERSION, orbit, app, img];</script>
<script type="module" src="./second.js"></script>
<script type="text/babel">const el = <b>hi</b>;</script>
<script src="https://unpkg.com/@babel/standalone/babel.min.js"></script>
</body></html>"#
    );
    let dir = site(
        "full",
        &[
            ("index.html", page.as_bytes()),
            ("app.js", b"export const app = 'app-module';\nexport const img = 'pic.png';\n"),
            ("second.js", b"window.second = true;"),
            ("local.css", b"@import 'nested.css';\na { color: red }"),
            ("nested.css", b"b { color: blue }"),
            ("pic.png", PNG),
            ("pic2x.png", PNG),
            (".secret/key.png", PNG),
        ],
    );
    std::fs::write(dir.parent().unwrap().join("outside.png"), PNG).unwrap();
    let cache = dir.join("..").join(format!("forge-pack-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);

    let summary = pack::run(&dir.join("index.html"), None, options(&base, &cache)).await.unwrap();
    let out = std::fs::read_to_string(dir.join("index.packed.html")).unwrap();
    assert_eq!(summary.output, dir.join("index.packed.html"));

    // Nothing points outside the file any more.
    for gone in [
        "unpkg.com",
        "jsdelivr",
        &base,
        "importmap",
        "babel.min.js",
        "Content-Security-Policy",
        "preload",
        "nomodule",
        "integrity",
        "srcset",
    ] {
        assert!(!out.contains(gone), "`{gone}` left in output:\n{out}");
    }
    // One module: both module scripts, the import map, CDN files and the addon resolving
    // back to the same `mini3d` module (one copy).
    assert_eq!(out.matches(r#"<script type="module">"#).count(), 1, "{out}");
    assert_eq!(out.matches("mini3d-1").count(), 1, "{out}");
    assert!(out.contains("orbit+") && out.contains("app-module") && out.contains("window.second = true"), "{out}");
    // Classic scripts inline and in order; deferred ones and compiled JSX at the end of <body>.
    assert!(out.contains("window.Mini3D = { umd: 'mini3d-umd' }"), "{out}");
    let (lib, jsx, body_end) = (
        out.find("remote-lib").unwrap(),
        out.find(r#"React.createElement("b""#).unwrap(),
        out.rfind("</body>").unwrap(),
    );
    assert!(out.find("mini3d-umd").unwrap() < out.find("<body>").unwrap() && lib < jsx && jsx < body_end, "{out}");
    // Assets, fonts and styles are embedded.
    let png = format!("data:image/png;base64,{}", b64(PNG));
    assert!(
        out.matches(&png).count() >= 5,
        "local + remote <img>, style attribute, CDN css url(), app.js literal:\n{out}"
    );
    assert!(out.contains("data:font/woff2;base64,"), "{out}");
    assert!(out.contains("b { color: blue }") && out.contains("a { color: red }"), "{out}");
    assert!(!out.contains(r#"media="print""#), "lazy-loaded CSS applies immediately:\n{out}");
    // Hidden files and files outside the folder are never read.
    assert!(out.contains(r#"src=".secret/key.png""#) && out.contains(r#"src="../outside.png""#), "{out}");
    let warnings = summary.warnings.join("\n");
    assert!(
        warnings.contains("hidden")
            && warnings.contains("outside the page's folder")
            && warnings.contains("Content-Security-Policy"),
        "{warnings}"
    );
    assert!(summary.packages.iter().any(|p| p == "mini3d@1.0.0"), "{:?}", summary.packages);

    // Offline, from the caches only, with the servers gone.
    server.abort();
    let _ = server.await;
    let mut offline = options(&base, &cache);
    offline.offline = true;
    pack::run(&dir.join("index.html"), Some(&dir.join("offline.html")), offline).await.unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("offline.html")).unwrap(), out);

    // A mismatched integrity attribute is an error, not a silent inline.
    let tampered = page.replace(&sri, "sha384-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    std::fs::write(dir.join("tampered.html"), tampered).unwrap();
    let mut offline = options(&base, &cache);
    offline.offline = true;
    let err = pack::run(&dir.join("tampered.html"), None, offline).await.unwrap_err();
    assert!(format!("{err:#}").contains("integrity"), "{err:#}");
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&cache).ok();
    std::fs::remove_file(dir.parent().unwrap().join("outside.png")).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn large_classic_scripts_and_styles_are_compressed_in_place() {
    let big_js: String = (0..3000).map(|i| format!("window.v{i} = {i};\n")).collect();
    let big_css: String = (0..3000).map(|i| format!(".c{i} {{ margin: {i}px }}\n")).collect();
    let page = "<html><head><link rel=\"stylesheet\" href=\"big.css\"></head><body><script src=\"big.js\"></script><script>window.after = window.v2999;</script></body></html>";
    let dir = site(
        "compress",
        &[("index.html", page.as_bytes()), ("big.js", big_js.as_bytes()), ("big.css", big_css.as_bytes())],
    );
    let cache = dir.join("cache-unused");
    pack::run(&dir.join("index.html"), None, options("http://127.0.0.1:9", &cache)).await.unwrap();
    let out = std::fs::read_to_string(dir.join("index.packed.html")).unwrap();
    assert!(out.len() * 3 < big_js.len() + big_css.len(), "{} vs {}", out.len(), big_js.len() + big_css.len());
    // The inflater is defined once, before the first compressed element (the stylesheet).
    assert_eq!(out.matches("__forgeInflateBytes=").count(), 1);
    assert!(out.find("__forgeInflateBytes=").unwrap() < out.find(r#"createElement("style")"#).unwrap());
    // The inline script after the compressed one is untouched, so it runs after it.
    assert!(out.find(r#"createElement("script")"#).unwrap() < out.find("window.after = window.v2999").unwrap());
    std::fs::remove_dir_all(&dir).ok();
}
