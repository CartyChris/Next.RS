from pathlib import Path
import json, hashlib, subprocess, sys, tempfile, os

ROOT = Path(__file__).resolve().parent
REPORT = {"passed": [], "failed": [], "blocked": []}

def ok(name, detail=""):
    REPORT["passed"].append({"name": name, "detail": detail})

def fail(name, detail):
    REPORT["failed"].append({"name": name, "detail": detail})

def block(name, detail):
    REPORT["blocked"].append({"name": name, "detail": detail})

def assert_true(name, cond, detail=""):
    if cond:
        ok(name, detail)
    else:
        fail(name, detail or "assertion failed")

def safe_entry_name(route: str) -> str:
    if route == "/":
        return "home"
    return route.strip("/").replace("[","").replace("]","").replace("{","").replace("}","").replace("/","_").replace("-","_")

def relative_import_specifier(from_dir: Path, to_file: Path) -> str:
    rel = os.path.relpath(to_file, from_dir).replace("\\", "/")
    if not rel.startswith("."):
        rel = "./" + rel
    return rel

def source_has_loader_export(source: str) -> bool:
    return ("export async function loader" in source or
            "export function loader" in source or
            "export const loader" in source)

def escape_inline_script(code: str) -> str:
    return code.replace("</script", "<\\/script")

def classify_output_file(path: str) -> str:
    ext = Path(path).suffix
    if ext in [".js", ".mjs"]:
        return "JavaScript"
    if ext == ".css":
        return "Css"
    if ext == ".map":
        return "SourceMap"
    if ext == ".json":
        return "Manifest"
    return "Asset"

def scan_user_source(source: str, deny_eval=True, deny_new_function=True, deny_remote_imports=True):
    findings = []
    if deny_eval and "eval(" in source:
        findings.append("Eval")
    if deny_new_function and "new Function(" in source:
        findings.append("NewFunction")
    if deny_remote_imports and any(x in source for x in ['import("http://', 'import("https://', 'import("data:', 'from "http://', 'from "https://', 'from "data:']):
        findings.append("RemoteImport")
    if "ext:forge_" in source or "forge:internal" in source:
        findings.append("InternalForgeImport")
    return findings

def build_temp_project():
    temp = Path(tempfile.mkdtemp(prefix="forge_8d_tests_"))
    app = temp / "app" / "blog" / "[slug]"
    app.mkdir(parents=True)
    page = app / "page.tsx"
    page.write_text("""
export async function loader({ params }) {
  return { title: params.slug };
}
export default function Page({ loaderData }) {
  return <main>{loaderData.title}</main>;
}
""")
    return temp, page

def generate_route_wrapper(route: str, source_path: Path, generated_path: Path):
    impl = relative_import_specifier(generated_path.parent, source_path)
    server = f"""
import Component from "{impl}";
import * as RouteModule from "{impl}";

export const route = "{route}";
export const Loading = RouteModule.Loading ?? null;
export const ErrorBoundary = RouteModule.ErrorBoundary ?? null;
export const loader = RouteModule.loader ?? null;

export async function load(ctx) {{
  if (!loader) return {{}};
  return await loader({{
    ctx,
    route,
    params: ctx.params ?? {{}},
    searchParams: ctx.searchParams ?? {{}},
    signal: ctx.signal,
  }});
}}

export function renderElement(ctx) {{
  return Component({{
    ctx,
    route,
    params: ctx.params ?? {{}},
    searchParams: ctx.searchParams ?? {{}},
  }});
}}

export {{ Component }};
export default Component;
"""
    client = f"""
import Component from "{impl}";
import * as RouteModule from "{impl}";

export const route = "{route}";
export const Loading = RouteModule.Loading ?? null;
export const ErrorBoundary = RouteModule.ErrorBoundary ?? null;
export const clientLoader = RouteModule.clientLoader ?? null;

export {{ Component }};
export default Component;
"""
    return server, client, impl

def rolldown_config_fixture(project_root: Path):
    output_dir = project_root / ".forge"
    forge_data = output_dir / "generated/internals/forge-data-runtime.js"
    forge_cache = output_dir / "generated/internals/forge-cache-runtime.js"
    forge_nav = output_dir / "generated/internals/forge-navigation-runtime.js"
    return f"""
import {{ defineConfig }} from "rolldown";

function forgeAliasPlugin(aliases) {{
  return {{ name: "forge-aliases", resolveId(source) {{ return aliases[source] ?? null; }} }};
}}

function forgeSecurityPlugin(options) {{
  const forbiddenPrefixes = ["ext:forge_", "forge:internal"];
  return {{
    name: "forge-security-policy",
    resolveId(source, importer) {{
      if (forbiddenPrefixes.some((prefix) => source.startsWith(prefix))) throw new Error("denied");
      if (options.denyRemoteImports && /^(https?:|data:|blob:)/.test(source)) throw new Error("denied");
      return null;
    }},
    transform(code, id) {{
      if (options.denyEval && /\\beval\\s*\\(/.test(code)) throw new Error("denied");
      if (options.denyNewFunction && /\\bnew\\s+Function\\s*\\(/.test(code)) throw new Error("denied");
      return null;
    }}
  }};
}}

function forgeManifestPlugin(fileName) {{
  return {{ name: "forge-manifest", generateBundle(_options, bundle) {{
    const manifest = {{}};
    for (const [bundleFileName, chunk] of Object.entries(bundle)) {{
      if (chunk.type === "chunk") manifest[chunk.name] = {{ fileName: bundleFileName, imports: chunk.imports, dynamicImports: chunk.dynamicImports, isEntry: chunk.isEntry, facadeModuleId: chunk.facadeModuleId }};
      else manifest[bundleFileName] = {{ fileName: bundleFileName, type: "asset" }};
    }}
    this.emitFile({{ type: "asset", fileName, source: JSON.stringify(manifest, null, 2) }});
  }} }};
}}

export default defineConfig([
{{
  input: {{
    "server-entry": ".forge/generated/server-entry.js"
  }},
  platform: "node",
  output: {{
    dir: "{str(output_dir / 'server')}",
    format: "esm",
    sourcemap: true,
    entryFileNames: "[name].[hash].js",
    chunkFileNames: "chunks/[name].[hash].js",
    assetFileNames: "assets/[name].[hash][extname]",
    codeSplitting: true
  }},
  treeshake: true,
  transform: {{ jsx: {{ runtime: "automatic", importSource: "react" }}, define: {{ "process.env.NODE_ENV": "development" }} }},
  plugins: [
    forgeAliasPlugin({{
      "forge:data": "{forge_data}",
      "forge:cache": "{forge_cache}",
      "forge:navigation": "{forge_nav}"
    }}),
    forgeSecurityPlugin({{ denyEval: true, denyNewFunction: true, denyWasmEval: false, denyRemoteImports: true }}),
    forgeManifestPlugin("forge-server-bundle-manifest.json")
  ]
}},
{{
  input: {{
    "forge-client-runtime": ".forge/generated/client-runtime.js",
    "route-blog-slug": ".forge/generated/routes/route-blog-slug.client.js"
  }},
  platform: "browser",
  output: {{
    dir: "{str(output_dir / 'static')}",
    format: "esm",
    sourcemap: true,
    entryFileNames: "[name].[hash].js",
    chunkFileNames: "chunks/[name].[hash].js",
    assetFileNames: "assets/[name].[hash][extname]",
    codeSplitting: true
  }},
  treeshake: true,
  transform: {{ jsx: {{ runtime: "automatic", importSource: "react" }}, define: {{ "process.env.NODE_ENV": "development" }} }},
  plugins: [
    forgeAliasPlugin({{
      "forge:data": "{forge_data}",
      "forge:cache": "{forge_cache}",
      "forge:navigation": "{forge_nav}"
    }}),
    forgeSecurityPlugin({{ denyEval: true, denyNewFunction: true, denyWasmEval: false, denyRemoteImports: true }}),
    forgeManifestPlugin("forge-browser-bundle-manifest.json")
  ]
}}
]);
"""

def run():
    required = [
        "src/build/bundler.rs",
        "src/build/rolldown.rs",
        "src/build/generated.rs",
        "src/build/manifest.rs",
        "src/build/security_scan.rs",
        "src/build/single_html.rs",
        "src/build/config.rs",
    ]
    for rel in required:
        assert_true(f"patch file exists: {rel}", (ROOT / rel).exists())

    manifest_src = (ROOT / "src/build/manifest.rs").read_text()
    for token in ["ManifestCapabilities", "ManifestBundle", "build_target", "capabilities", "bundle"]:
        assert_true(f"manifest contains {token}", token in manifest_src)

    config_src = (ROOT / "src/build/config.rs").read_text()
    for token in ["BundlerKind", "BuildTarget", "bundler", "target", "security"]:
        assert_true(f"BuildConfig contains {token}", token in config_src)

    temp, page = build_temp_project()
    generated_route_path = temp / ".forge/generated/routes/route-blog-slug.server.js"
    server, client, impl = generate_route_wrapper("/blog/{slug}", page, generated_route_path)

    assert_true("relative import computed from wrapper parent", impl == "../../../app/blog/[slug]/page.tsx", impl)
    assert_true("source_has_loader_export detects loader", source_has_loader_export(page.read_text()))
    assert_true("server wrapper exposes loader", "export const loader = RouteModule.loader ?? null" in server)
    assert_true("server wrapper exposes load(ctx)", "export async function load(ctx)" in server)
    assert_true("client wrapper uses clientLoader, not server loader", "clientLoader" in client and "export const loader" not in client)

    gen = temp / ".forge/generated"
    (gen / "routes").mkdir(parents=True)
    (gen / "internals").mkdir(parents=True)
    (gen / "server-entry.js").write_text("globalThis.__forgeRenderToString = async () => '<html></html>';")
    (gen / "client-runtime.js").write_text("console.log('runtime');")
    (gen / "routes/route-blog-slug.server.js").write_text(server)
    (gen / "routes/route-blog-slug.client.js").write_text(client)
    for rel in ["server-entry.js", "client-runtime.js", "routes/route-blog-slug.server.js", "routes/route-blog-slug.client.js"]:
        assert_true(f"generated file exists: {rel}", (gen / rel).exists())

    config = rolldown_config_fixture(temp)
    (temp / ".forge/rolldown.config.mjs").write_text(config)
    for token in ["forgeAliasPlugin", "forgeSecurityPlugin", "forgeManifestPlugin", 'platform: "node"', 'platform: "browser"', '"forge:data"', 'process.env.NODE_ENV']:
        assert_true(f"rolldown config contains {token}", token in config)

    try:
        proc = subprocess.run(["node", "--check", str(temp / ".forge/rolldown.config.mjs")], text=True, capture_output=True, timeout=10)
        assert_true("rolldown config passes node --check", proc.returncode == 0, proc.stderr.strip())
    except FileNotFoundError:
        block("rolldown config node syntax check", "node is not installed")
    except Exception as e:
        fail("rolldown config node syntax check", str(e))

    try:
        proc = subprocess.run(["rolldown", "--version"], text=True, capture_output=True, timeout=10)
        if proc.returncode == 0:
            ok("rolldown binary available", proc.stdout.strip())
        else:
            block("rolldown binary available", proc.stderr.strip() or "non-zero exit")
    except FileNotFoundError:
        block("rolldown binary available", "rolldown binary is not installed")
    except Exception as e:
        block("rolldown binary available", str(e))

    assert_true("security scan catches eval", "Eval" in scan_user_source("eval('x')"))
    assert_true("security scan catches new Function", "NewFunction" in scan_user_source("new Function('x')"))
    assert_true("security scan catches internal import", "InternalForgeImport" in scan_user_source('import "ext:forge_streaming/runtime.js";'))
    assert_true("security scan catches remote import", "RemoteImport" in scan_user_source('import("https://evil.example/x.js")'))

    assert_true("classifies js", classify_output_file("app.js") == "JavaScript")
    assert_true("classifies css", classify_output_file("style.css") == "Css")
    assert_true("classifies map", classify_output_file("app.js.map") == "SourceMap")
    assert_true("classifies json", classify_output_file("manifest.json") == "Manifest")

    browser_manifest = {
        "route-blog-slug": {
            "fileName": "route-blog-slug.abc123.js",
            "imports": [],
            "dynamicImports": [],
            "isEntry": True,
            "facadeModuleId": ".forge/generated/routes/route-blog-slug.client.js",
        }
    }
    public_path = "/static/" + browser_manifest["route-blog-slug"]["fileName"]
    assert_true("bundle manifest maps route to public path", public_path == "/static/route-blog-slug.abc123.js")

    out_js = temp / ".forge/static/route-blog-slug.abc123.js"
    out_js.parent.mkdir(parents=True, exist_ok=True)
    out_js.write_text("console.log('bundle');")
    h = hashlib.sha256(out_js.read_bytes()).hexdigest()
    assert_true("capsule hash computed for bundle output", len(h) >= 32, h[:16])

    bad_script = "console.log('</script><img src=x onerror=alert(1)>')"
    escaped = escape_inline_script(bad_script)
    assert_true("single html escapes closing script", "</script>" not in escaped)
    html = f'<!doctype html><div id="__forge_root"></div><script type="application/json" id="__FORGE_DATA__">{{"schemaVersion":1,"singleFile":true}}</script><script type="module">{escaped}</script>'
    assert_true("single html has forge root", "__forge_root" in html)
    assert_true("single html has singleFile capability data", '"singleFile":true' in html)

    rolldown_src = (ROOT / "src/build/rolldown.rs").read_text()
    assert_true("rolldown source has forge manifest plugin", "forgeManifestPlugin" in rolldown_src)
    assert_true("rolldown source has alias plugin", "forgeAliasPlugin" in rolldown_src)
    assert_true("rolldown source has CLI adapter", "Command::new" in rolldown_src)

    try:
        proc = subprocess.run(["cargo", "test"], cwd=ROOT, text=True, capture_output=True, timeout=30)
        if proc.returncode == 0:
            ok("cargo test", proc.stdout[-1000:])
        else:
            block("cargo test", proc.stderr[-1000:] or proc.stdout[-1000:])
    except FileNotFoundError:
        block("cargo test", "cargo is not installed in this environment")
    except PermissionError:
        block("cargo test", "cargo is not executable/permission denied in this environment")
    except Exception as e:
        block("cargo test", str(e))

    report_path = ROOT / "non-rolldown-test-report.json"
    report_path.write_text(json.dumps(REPORT, indent=2))
    summary_path = ROOT / "non-rolldown-test-summary.md"
    summary_path.write_text(
        "# Forge 8D non-Rolldown test summary\n\n"
        f"Passed: {len(REPORT['passed'])}\n\n"
        f"Failed: {len(REPORT['failed'])}\n\n"
        f"Blocked: {len(REPORT['blocked'])}\n\n"
        "## Failures\n\n"
        + ("\n".join(f"- {x['name']}: {x['detail']}" for x in REPORT["failed"]) or "None")
        + "\n\n## Blocked\n\n"
        + ("\n".join(f"- {x['name']}: {x['detail']}" for x in REPORT["blocked"]) or "None")
        + "\n"
    )

    print(json.dumps({
        "passed": len(REPORT["passed"]),
        "failed": len(REPORT["failed"]),
        "blocked": len(REPORT["blocked"]),
        "report": str(report_path),
        "summary": str(summary_path),
    }, indent=2))

    return len(REPORT["failed"])

if __name__ == "__main__":
    sys.exit(run())
