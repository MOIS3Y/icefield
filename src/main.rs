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
use console::style;
use lua_engine::LuaEngine;

/// Application entry point.
///
/// Coordinates the execution flow by parsing CLI arguments, initializing
/// the logging system, resolving paths, and dispatching commands.
fn main() -> anyhow::Result<()> {
    // Phase 0: Initialization
    let cli = Cli::parse();

    let (default_config_dir, default_state_path, default_cache_dir) =
        paths::get_default_paths();

    let config_path = cli
        .config
        .unwrap_or_else(|| default_config_dir.join(paths::CONFIG_FILE));
    let state_path = cli.state.unwrap_or(default_state_path);
    let log_dir = state_path.parent().unwrap_or(&default_cache_dir);

    // Initialize logging (logs go to icefield.log in the same dir as state)
    let _log_guard = logging::setup(cli.verbose, log_dir);

    // Command dispatching
    match cli.command {
        Commands::Apply { dry_run, force } => {
            if dry_run {
                println!(
                    "{} {}",
                    style("❄").blue(),
                    style("Dry run mode enabled. No changes will be made.")
                        .dim()
                );
            }

            if force {
                println!(
                    "{} {}",
                    style("!").yellow(),
                    style("Force mode enabled. Cache optimization will be bypassed.")
                        .yellow()
                );
            }

            let derivations = LuaEngine::load_file(&config_path)?;

            if dry_run {
                // Phase 2: Build (in-memory simulation for dry run)
                for der in derivations {
                    println!(
                        "  {} {}",
                        style("🔨").dim(),
                        style(&der.meta.name).dim()
                    );
                }
            } else {
                // Phase 2 & 3: Build and Commit
                let applier = Applier::new(state_path);
                applier.apply(&derivations, force)?;
            }
        }
        Commands::Gc => {
            let derivations = LuaEngine::load_file(&config_path)?;
            let applier = Applier::new(state_path);
            applier.gc(&derivations)?;
        }
        Commands::Status => {
            // Displays the current state of managed files from state.json
            let state = state::State::load(&state_path)?;
            println!(
                "{} Found {} managed files",
                style("❄").blue(),
                style(state.managed_files.len()).bold()
            );
            for (path, hash) in state.managed_files {
                println!(
                    "  {} {} {}",
                    style("•").dim(),
                    style(path.display()).cyan(),
                    style(format!("[{}]", &hash[..8])).dim()
                );
            }
        }
    }

    Ok(())
}
