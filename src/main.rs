use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use nextrs::{
    build::{self, manifest::RouteEntry},
    config::{Mode, Project, Target},
    routes::RouteKind,
    scaffold,
    server::{self, App, Server},
};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(
    name = "forge",
    version,
    about = "Next.RS: Rust-native React framework (SSR server, static site, or one HTML file)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new project.
    New {
        dir: PathBuf,
        /// starter (app shell on forge:ui), react (TSX), jsx, vanilla, three (three.js game), ai (streaming chat),
        /// pixel (self-saving single file), arena (multiplayer + neural bots), models (Rust/WASM 3D models)
        #[arg(long, short, default_value = "react")]
        template: String,
    },
    /// Development server with rebuild-on-save and live reload.
    Dev {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, short)]
        port: Option<u16>,
        #[arg(long)]
        host: Option<String>,
        /// server (SSR, default) | single-html (serve the real single file, rebuilt on save)
        #[arg(long, short, default_value = "server")]
        target: Target,
    },
    /// Production build.
    Build {
        #[arg(default_value = ".")]
        root: PathBuf,
        /// server | static | single-html
        #[arg(long, short, default_value = "server")]
        target: Target,
        /// Development build (no minification, sourcemaps).
        #[arg(long)]
        dev: bool,
    },
    /// Serve a `server` build from .forge/.
    Start {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, short)]
        port: Option<u16>,
        #[arg(long)]
        host: Option<String>,
    },
    /// Validate routes, compile configured Rust/WASM and scan both bundles without deployable output.
    Check {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// Run a TypeScript or JavaScript file in the embedded engine (the same sandbox as server
    /// code: `fetch` reaches only `server.allowFetch` hosts). A default-exported function is
    /// called and its result printed.
    Run {
        file: PathBuf,
        /// Project directory, for forge.config.json and node_modules (default: the file's folder).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Run `*.test.*` and `*.spec.*` files with the built-in `forge:test` API.
    Test {
        /// `[dir] [filter]`: the project directory (default: the current one), then text
        /// that a test file's path must contain. `forge test loader` filters in the
        /// current project.
        #[arg(num_args = 0..=2)]
        args: Vec<String>,
    },
    /// Model Context Protocol server for coding agents: built-in docs, forge:ui lookup,
    /// and tools to create, check, build and pack projects. Speaks stdio by default.
    Mcp {
        /// Serve Streamable HTTP on this port (POST /mcp) instead of stdio, for clients
        /// that connect over the network such as n8n's MCP Client node.
        #[arg(long)]
        http: Option<u16>,
        /// Interface for --http.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Search the documentation built into this binary, or print one document.
    Docs {
        /// Words to search for. Without a query, lists the documents.
        query: Vec<String>,
        /// Print this whole document: agents, guide, ui, design, architecture.
        #[arg(long)]
        read: Option<String>,
    },
    /// Find a `forge:ui` look (theme preset) from a description, or list them all.
    Themes {
        /// Words describing the look: "frosted glass", "dense admin", "retro console".
        query: Vec<String>,
    },
    /// One offline HTML file from an HTML page or a component file (`App.tsx`): CDN scripts
    /// and imports come from npm (verified), everything else is downloaded, and all of it is
    /// inlined and compressed.
    #[cfg(feature = "pack")]
    Pack {
        /// An `.html` page, or a `.jsx` / `.tsx` / `.js` / `.ts` component or script.
        input: PathBuf,
        /// Output file (default: `<name>.packed.html` for pages, `<name>.html` for components).
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Components: page title (default: from the file name).
        #[arg(long)]
        title: Option<String>,
        /// Components: add Tailwind's browser build (default: when the markup uses Tailwind classes).
        #[arg(long, conflicts_with = "no_tailwind")]
        tailwind: bool,
        /// Components: never add Tailwind.
        #[arg(long)]
        no_tailwind: bool,
        /// auto | on | off: compress large scripts and styles.
        #[arg(long, default_value = "auto")]
        compress: String,
        /// Keep bundled module code unminified.
        #[arg(long)]
        no_minify: bool,
        /// Use only the package and download caches.
        #[arg(long)]
        offline: bool,
        /// npm registry (default: $FORGE_NPM_REGISTRY, $npm_config_registry, or registry.npmjs.org).
        #[arg(long)]
        registry: Option<String>,
        /// Allow fetching from localhost and private networks (off by default: a packed page
        /// must not embed internal services or cloud metadata).
        #[arg(long)]
        allow_private_network: bool,
        /// Repack on every change and serve the result with live reload.
        #[arg(long, short)]
        watch: bool,
        /// Port for --watch (default 3000).
        #[arg(long, short)]
        port: Option<u16>,
    },
}

fn addr(project: &Project, host: Option<String>, port: Option<u16>) -> Result<SocketAddr> {
    let host = host.unwrap_or_else(|| project.config.server.host.clone());
    let port = port.or_else(|| std::env::var("PORT").ok()?.parse().ok()).unwrap_or(project.config.server.port);
    format!("{host}:{port}")
        .parse()
        .or_else(|_| format!("[{host}]:{port}").parse())
        .with_context(|| format!("invalid address {host}:{port}"))
}

fn print_routes(routes: &[RouteEntry], not_found: Option<&RouteEntry>) {
    for r in routes.iter().chain(not_found) {
        let pattern = if r.pattern.is_empty() { "(not found)" } else { &r.pattern };
        let mut tags = Vec::new();
        match r.kind {
            RouteKind::Api => tags.push("api".to_string()),
            RouteKind::Page => {
                tags.push(r.render.map(|k| k.as_str()).unwrap_or("react").to_string());
                tags.push(if r.ssr { "ssr" } else { "client-only" }.into());
                if r.loader {
                    tags.push("loader".into());
                }
                if let Some(s) = r.revalidate {
                    tags.push(format!("revalidate {s}s"));
                }
            }
        }
        println!("  {pattern:<32} {:<28} {}", tags.join(" · "), r.file);
    }
}

fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / 1048576.0),
        b if b >= 1 << 10 => format!("{:.1} kB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

#[tokio::main]
async fn main() {
    if let Err(e) = run(Cli::parse()).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::New { dir, template } => {
            scaffold::create(&dir, &template)?;
            println!("Created {} ({template}).\n\n  cd {}\n  npm install\n  forge dev\n", dir.display(), dir.display());
            if template == "vanilla" {
                println!("(The vanilla template has no npm dependencies; `npm install` is optional.)");
            }
            if template == "models" {
                println!(
                    "Rust models require Cargo and `rustup target add wasm32-unknown-unknown`. Use `npm ci` for the locked example dependencies."
                );
            }
        }
        Command::Dev { root, port, host, target } => {
            let project = Project::load(&root, Target::Server, Mode::Dev)?;
            match target {
                Target::SingleHtml => {
                    server::single::run_project(project.root.clone(), addr(&project, host, port)?).await?
                }
                Target::Server => server::dev::run(project.root.clone(), addr(&project, host, port)?).await?,
                other => anyhow::bail!("forge dev serves the server or single-html target, not {}", other.as_str()),
            }
        }
        Command::Build { root, target, dev } => {
            let project = Project::load(&root, target, if dev { Mode::Dev } else { Mode::Prod })?;
            let result = build::build(&project).await?;
            for w in &result.warnings {
                eprintln!("warn: {w}");
            }
            println!("Routes:");
            print_routes(&result.manifest.routes, result.manifest.not_found.as_ref());
            let s = &result.stats;
            println!(
                "\nClient JS {} ({} gzip) · CSS {} · assets {}",
                human(s.client_bytes as u64),
                human(s.client_gzip as u64),
                human(s.css_bytes as u64),
                human(s.asset_bytes as u64)
            );
            if !result.artifacts.is_empty() {
                let total: u64 = result.artifacts.iter().map(|(_, n)| n).sum();
                if target == Target::SingleHtml {
                    println!("→ {} ({})", result.artifacts[0].0.display(), human(total));
                } else {
                    println!(
                        "→ {} files, {} in {}",
                        result.artifacts.len(),
                        human(total),
                        project.dist_dir().display()
                    );
                }
            } else {
                println!("→ {}", project.out_dir().display());
            }
            println!("Built in {} ms", result.elapsed.as_millis());
        }
        Command::Start { root, port, host } => {
            let project = Project::load(&root, Target::Server, Mode::Prod)?;
            let addr = addr(&project, host, port)?;
            let app = App::load(project).await?;
            let workers = app.pool.len();
            let server = Server::new(false);
            server.set_app(app);
            println!("forge start → http://{addr} ({workers} JS workers)");
            server.serve(addr).await?;
        }
        #[cfg(feature = "pack")]
        Command::Pack {
            input,
            output,
            title,
            tailwind,
            no_tailwind,
            compress,
            no_minify,
            offline,
            registry,
            allow_private_network,
            watch,
            port,
        } => {
            let compress = match compress.as_str() {
                "auto" => nextrs::config::Compress::Auto,
                "on" | "true" => nextrs::config::Compress::On,
                "off" | "false" => nextrs::config::Compress::Off,
                other => anyhow::bail!("--compress takes auto, on or off (got `{other}`)"),
            };
            let tailwind = if tailwind {
                Some(true)
            } else if no_tailwind {
                Some(false)
            } else {
                None
            };
            let mut options = nextrs::pack::Options {
                compress,
                minify: !no_minify,
                offline,
                tailwind,
                title,
                allow_private_network,
                ..Default::default()
            };
            if let Some(r) = registry {
                options.registry = r;
            }
            if watch {
                let port = port.or_else(|| std::env::var("PORT").ok()?.parse().ok()).unwrap_or(3000);
                return nextrs::pack::watch(input, output, options, SocketAddr::from(([127, 0, 0, 1], port))).await;
            }
            let started = std::time::Instant::now();
            let s = nextrs::pack::run(&input, output.as_deref(), options).await?;
            for (what, from) in &s.sources {
                println!("  {from:<12} {what}");
            }
            for p in &s.packages {
                println!("  installed    {p}");
            }
            for n in &s.notes {
                println!("  note: {n}");
            }
            for w in &s.warnings {
                eprintln!("warn: {w}");
            }
            println!(
                "→ {} ({}, from a {} page) in {} ms",
                s.output.display(),
                human(s.bytes as u64),
                human(s.input_bytes as u64),
                started.elapsed().as_millis()
            );
        }
        Command::Run { file, root } => {
            let dir = root.unwrap_or_else(|| {
                file.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| ".".into(), PathBuf::from)
            });
            let project = Project::load(&dir, Target::Server, Mode::Prod)?;
            if let Some(output) = build::script::run(&project, &file).await? {
                println!("{output}");
            }
        }
        Command::Test { args } => {
            // A first argument that is not a directory is the filter.
            let (root, filter) = match args.as_slice() {
                [] => (PathBuf::from("."), None),
                [one] if std::path::Path::new(one).is_dir() => (PathBuf::from(one), None),
                [one] => (PathBuf::from("."), Some(one.clone())),
                [dir, filter, ..] => (PathBuf::from(dir), Some(filter.clone())),
            };
            let project = Project::load(&root, Target::Server, Mode::Prod)?;
            let files = build::script::find_tests(&project.root, filter.as_deref());
            if files.is_empty() {
                anyhow::bail!(
                    "no test files found: name them *.test.ts (or .tsx, .js, .jsx) and import {{ test, expect }} from \"forge:test\""
                );
            }
            let started = std::time::Instant::now();
            let results = build::script::test(&project, &files).await?;
            let mut file = "";
            let (mut passed, mut failed, mut skipped) = (0, 0, 0);
            for r in &results {
                if r.file != file {
                    file = &r.file;
                    println!("\n{file}");
                }
                if r.skipped {
                    skipped += 1;
                    println!("  - {} (skipped)", r.name);
                } else if r.ok {
                    passed += 1;
                    println!("  \x1b[32m✓\x1b[0m {} \x1b[2m{:.0} ms\x1b[0m", r.name, r.ms);
                } else {
                    failed += 1;
                    println!("  \x1b[31m✗\x1b[0m {}\n      {}", r.name, r.error);
                    for line in r.stack.lines().skip(1) {
                        println!("      {}", line.trim());
                    }
                }
            }
            let skip = if skipped > 0 { format!(", {skipped} skipped") } else { String::new() };
            println!("\n{passed} passed, {failed} failed{skip} in {} ms", started.elapsed().as_millis());
            if failed > 0 {
                std::process::exit(1);
            }
        }
        Command::Mcp { http, host } => match http {
            Some(port) => nextrs::mcp::serve_http(&host, port).await?,
            None => nextrs::mcp::serve_stdio().await?,
        },
        Command::Docs { query, read } => {
            if let Some(name) = read {
                let doc = nextrs::mcp::DOCS.iter().find(|d| d.0 == name);
                let doc = doc.with_context(|| {
                    format!("unknown document `{name}`: try agents, guide, ui, design, architecture")
                })?;
                println!("{}", doc.2);
            } else if query.is_empty() {
                for (name, about, _) in nextrs::mcp::DOCS {
                    println!("{name:<13} {about}");
                }
                println!("\nforge docs <words>      search\nforge docs --read ui    print one document");
            } else {
                let ranked = nextrs::mcp::search(&query.join(" "), None);
                if ranked.is_empty() {
                    println!("No section matches. Try fewer or different words.");
                }
                for (_, section) in ranked.iter().take(3) {
                    println!("## {} › {}\n\n{}\n", section.doc, section.heading, section.body.trim());
                }
            }
        }
        Command::Themes { query } => {
            let query = query.join(" ");
            let (text, _) = nextrs::mcp::themes_text((!query.is_empty()).then_some(query.as_str()));
            println!("{text}");
        }
        Command::Check { root } => {
            let project = Project::load(&root, Target::Server, Mode::Prod)?;
            let (plan, warnings) = build::check(&project).await?;
            for w in &warnings {
                eprintln!("warn: {w}");
            }
            println!("Routes:");
            print_routes(&plan.routes, plan.not_found.as_ref());
            println!("\n✓ routes valid, security scan clean");
        }
    }
    Ok(())
}
