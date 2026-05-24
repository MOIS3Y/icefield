//! Icefield: A declarative dotfile manager powered by Rust and Lua.
//!
//! This crate provides a CLI tool that allows users to define their system
//! configuration in Lua and apply it atomically. It follows a three-phase
//! approach:
//! 1. **Compute**: Execute Lua to generate a graph of derivations.
//! 2. **Build**: Render templates and serialize data into final content.
//! 3. **Commit**: Synchronize the built content with the filesystem.

mod builder;
mod cli;
mod store;
mod inspector;
mod logging;
mod lua_api;
mod lua_engine;
mod model;
mod paths;
mod state;
mod switcher;
mod utils;

use clap::Parser;
use cli::{Cli, Commands};
use console::style;
use lua_engine::LuaEngine;
use std::path::PathBuf;
use switcher::Switcher;

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
    let state_path = cli.state.clone().unwrap_or(default_state_path);

    // Determine the base directory for cache (store/fetchers) and logs.
    // If state is overridden via CLI, we use its parent directory;
    // otherwise, we fall back to the system default cache directory.
    let base_cache_dir = if cli.state.is_some() {
        state_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        default_cache_dir
    };

    // Initialize logging (logs go to icefield.log in the base cache dir)
    let _log_guard = logging::setup(cli.verbose, &base_cache_dir);

    // Command dispatching
    match cli.command {
        Commands::Switch { dry_run, force } => {
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

            println!(
                "{} {}",
                style("❄").blue(),
                style("Computing configuration").bold()
            );

            let derivations =
                LuaEngine::load_file(&config_path, &base_cache_dir)?;

            // Phase 2 & 3: Build and Commit
            let switcher = Switcher::new(state_path);
            switcher.apply(&derivations, force)?;
        }
        Commands::Info => {
            inspector::inspect(&state_path)?;
        }
    }

    Ok(())
}
