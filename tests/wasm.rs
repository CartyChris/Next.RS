//! Real Rust compilation and packaging. Enable `e2e` and install the WASM target.

mod common;

use common::*;
use nextrs::config::Target;

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn wasm_manifest_cannot_escape_through_a_symlink() {
    let outside = project("wasm-outside", &[("Cargo.toml", "[package]\nname='outside'\nversion='0.1.0'\n")]);
    let root = project(
        "wasm-symlink",
        &[
            ("app/page.js", "export function mount() {}"),
            ("forge.config.json", r#"{"build":{"wasm":[{"name":"mesh","manifestPath":"rust/Cargo.toml"}]}}"#),
        ],
    );
    std::os::unix::fs::symlink(&outside, root.join("rust")).unwrap();
    let error = format!("{:#}", build(&root, Target::SingleHtml).await.unwrap_err());
    assert!(error.contains("outside the project"), "{error}");
    assert!(!outside.join("target").exists());
    std::fs::remove_dir_all(root).ok();
    std::fs::remove_dir_all(outside).ok();
}

#[cfg(feature = "e2e")]
mod compilation {
    use super::*;
    use std::path::PathBuf;

    const MANIFEST: &str = "[package]\nname='fixture-wasm'\nversion='0.1.0'\nedition='2024'\n[lib]\nname='custom_output_name'\ncrate-type=['cdylib']\n[features]\nextra=[]\n";
    const LOCK: &str = "version = 4\n[[package]]\nname = \"fixture-wasm\"\nversion = \"0.1.0\"\n";
    const SOURCE: &str =
        "#[unsafe(no_mangle)] pub extern \"C\" fn answer() -> u32 { if cfg!(feature=\"extra\") { 43 } else { 42 } }";

    fn fixture() -> PathBuf {
        project(
            "wasm-compile",
            &[
                (
                    "app/page.js",
                    "import wasm from 'forge:wasm/mesh'; export function mount(el) { el.dataset.wasm = wasm; }",
                ),
                (
                    "forge.config.json",
                    r#"{"build":{"compress":false,"wasm":[{"name":"mesh","manifestPath":"rust/mesh/Cargo.toml"}]}}"#,
                ),
                ("rust/mesh/Cargo.toml", MANIFEST),
                ("rust/mesh/Cargo.lock", LOCK),
                ("rust/mesh/src/lib.rs", SOURCE),
            ],
        )
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn compiles_a_renamed_cdylib_and_packages_all_browser_targets() {
        let root = fixture();
        let single = build(&root, Target::SingleHtml).await.unwrap();
        let wasm = std::fs::read(root.join(".forge/wasm/mesh.wasm")).unwrap();
        assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0", "a real WebAssembly module");
        assert_eq!(single.artifacts.len(), 1);
        assert_eq!(std::fs::read_dir(root.join("dist")).unwrap().count(), 1);
        let html = std::fs::read_to_string(root.join("dist/index.html")).unwrap();
        assert!(html.contains("data:application/wasm;base64,AGFzbQ"));
        assert!(!html.contains("forge:wasm/mesh"), "alias must resolve at build time");
        for (target, dir) in [(Target::Server, ".forge/client/assets"), (Target::Static, "dist/_forge/static/assets")] {
            build(&root, target).await.unwrap();
            let files: Vec<_> = std::fs::read_dir(root.join(dir))
                .unwrap()
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wasm"))
                .collect();
            assert_eq!(files.len(), 1);
            assert_eq!(std::fs::read(files[0].path()).unwrap(), wasm);
        }
        assert!(!root.join("rust/mesh/target").exists(), "Cargo artifacts stay out of watched sources");
        assert_eq!(std::fs::read_to_string(root.join("rust/mesh/Cargo.lock")).unwrap(), LOCK);
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn source_and_feature_changes_recompile_and_failures_never_use_stale_wasm() {
        let root = fixture();
        let first = build(&root, Target::Server).await.unwrap();
        let bytes = std::fs::read(root.join(".forge/wasm/mesh.wasm")).unwrap();
        std::fs::write(root.join("forge.config.json"), r#"{"build":{"wasm":[{"name":"mesh","manifestPath":"rust/mesh/Cargo.toml","features":["extra"],"defaultFeatures":false}]}}"#).unwrap();
        let changed = build(&root, Target::Server).await.unwrap();
        assert_ne!(bytes, std::fs::read(root.join(".forge/wasm/mesh.wasm")).unwrap());
        assert_ne!(first.manifest.build_id, changed.manifest.build_id);
        std::fs::write(
            root.join("rust/mesh/src/lib.rs"),
            "#[unsafe(no_mangle)] pub extern \"C\" fn answer() -> u32 { 99 }",
        )
        .unwrap();
        let source_changed = build(&root, Target::Server).await.unwrap();
        assert_eq!(changed.server_code, source_changed.server_code, "fixture changes only Rust client code");
        assert_ne!(changed.manifest.build_id, source_changed.manifest.build_id, "a Rust-only edit must trigger reload");
        std::fs::write(root.join("rust/mesh/src/lib.rs"), "compile_error!(\"intentional fixture failure\");").unwrap();
        let error = format!("{:#}", build(&root, Target::Server).await.unwrap_err());
        assert!(error.contains("mesh") && error.contains("intentional fixture failure"), "{error}");
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn missing_cdylib_and_unlocked_dependencies_are_actionable_errors() {
        let root = fixture();
        std::fs::write(
            root.join("rust/mesh/Cargo.toml"),
            "[package]\nname='fixture-wasm'\nversion='0.1.0'\nedition='2024'\n[lib]\ncrate-type=['rlib']\n",
        )
        .unwrap();
        let error = format!("{:#}", build(&root, Target::SingleHtml).await.unwrap_err());
        assert!(error.contains("cdylib"), "{error}");
        std::fs::write(root.join("rust/mesh/Cargo.toml"), MANIFEST).unwrap();
        std::fs::remove_file(root.join("rust/mesh/Cargo.lock")).unwrap();
        let error = format!("{:#}", build(&root, Target::SingleHtml).await.unwrap_err());
        assert!(error.contains("--locked") && error.contains("generate-lockfile"), "{error}");
        assert!(!root.join("rust/mesh/Cargo.lock").exists(), "Forge must not silently change dependency resolution");
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn wasm_output_cannot_follow_a_symlink() {
        let root = fixture();
        let outside = project("wasm-output-outside", &[("keep.txt", "untouched")]);
        std::fs::create_dir(root.join(".forge")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".forge/wasm")).unwrap();
        let error = format!("{:#}", build(&root, Target::SingleHtml).await.unwrap_err());
        assert!(error.contains("symlink"), "{error}");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(outside).ok();
    }
}
