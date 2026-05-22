//! Icefield: A declarative dotfile manager powered by Rust and Lua.
//!
//! This crate provides a CLI tool that allows users to define their system
//! configuration in Lua and apply it atomically. It follows a three-phase
//! approach:
//! 1. **Compute**: Execute Lua to generate a graph of derivations.
//! 2. **Build**: Render templates and serialize data into final content.
//! 3. **Commit**: Synchronize the built content with the filesystem.

mod applier;
mod builder;
mod cli;
mod logging;
mod lua_engine;
mod model;
mod paths;
mod state;
mod utils;

use applier::Applier;
use clap::Parser;
use cli::{Cli, Commands};
use lua_engine::LuaEngine;

/// Application entry point.
///
/// Coordinates the execution flow by parsing CLI arguments, initializing
/// the logging system, resolving paths, and dispatching commands.
fn main() -> anyhow::Result<()> {
    // Phase 0: Initialization
    let cli = Cli::parse();
    logging::setup(cli.verbose);

    let (default_config_dir, default_state_path) = paths::get_default_paths();
    let config_path = cli
        .config
        .unwrap_or_else(|| default_config_dir.join("init.lua"));
    let state_path = cli.state.unwrap_or(default_state_path);

    // Command dispatching
    match cli.command {
        Commands::Apply { dry_run } => {
            if dry_run {
                tracing::info!(
                    "Dry run mode enabled. No changes will be made."
                );
            }

            if !config_path.exists() {
                anyhow::bail!("Config file not found: {:?}", config_path);
            }

            // Phase 1: Compute Graph (Lua)
            let script = std::fs::read_to_string(&config_path)?;
            let engine =
                LuaEngine::new().map_err(|e| anyhow::anyhow!("{}", e))?;
            let derivations = engine
                .execute(&script)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            if dry_run {
                // Phase 2: Build (in-memory simulation for dry run)
                for der in derivations {
                    tracing::info!("Would apply: {}", der.meta.name);
                }
            } else {
                // Phase 2 & 3: Build and Commit
                let applier = Applier::new(state_path);
                applier.apply(&derivations)?;
            }
        }
        Commands::Gc => {
            tracing::info!(
                "Garbage collection not implemented as standalone yet."
            );
        }
        Commands::Status => {
            // Displays the current state of managed files from state.json
            let state = state::State::load(&state_path)?;
            tracing::info!("Managed files: {}", state.managed_files.len());
            for (path, hash) in state.managed_files {
                tracing::info!("  {:?} [{}]", path, hash);
            }
        }
    }

    Ok(())
}
