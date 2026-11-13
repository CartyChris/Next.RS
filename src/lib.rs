//! Next.RS / Forge: a Rust-native React meta-framework.
//!
//! * [`build`]: route scan → codegen → in-process Rolldown → manifest/capsule/stats
//! * [`runtime`]: embedded QuickJS worker pool that runs the server bundle
//! * [`server`]: Axum front end (static assets, SSR, `/_forge/data`, API routes, page cache)

pub mod build;
pub mod config;
#[cfg(feature = "pack")]
pub mod pack;
pub mod render;
pub mod routes;
pub mod runtime;
pub mod scaffold;
pub mod server;
