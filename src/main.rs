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
        /// react (TSX), jsx, vanilla, three (three.js game), ai (streaming chat), arena (multiplayer + neural bots)
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
    /// Validate routes and run the security scan without writing a build.
    Check {
        #[arg(default_value = ".")]
        root: PathBuf,
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
        }
        Command::Dev { root, port, host } => {
            let project = Project::load(&root, Target::Server, Mode::Dev)?;
            server::dev::run(project.root.clone(), addr(&project, host, port)?).await?;
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
