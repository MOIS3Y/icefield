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
mod inspector;
mod logging;
mod lua_api;
mod lua_engine;
mod lua_registry;
mod model;
mod paths;
mod state;
mod store;
mod switcher;
mod utils;

use clap::Parser;
use cli::{Cli, Commands};
use console::style;
use lua_engine::LuaEngine;
use switcher::Switcher;

/// Application entry point.
///
/// Coordinates the execution flow by parsing CLI arguments, initializing
/// the logging system, resolving paths, and dispatching commands.
fn main() -> anyhow::Result<()> {
    // Phase 0: Initialization
    let cli = Cli::parse();

    // Command dispatching
    match cli.command {
        Commands::Stubs => {
            // Generate Lua API stubs
            let paths = paths::AppPaths::resolve(None);
            let lua = mlua::Lua::new();
            let mut registry = lua_registry::ApiRegistry::new();
            lua_api::register(&lua, &paths, &mut registry)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("{}", registry.generate_stubs());
            return Ok(());
        }
        Commands::Switch { dry_run, force } => {
            let paths = paths::AppPaths::resolve(cli.config.clone());
            let _log_guard = logging::setup(cli.verbose, &paths);

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

            // Phase 1: Compute (Lua -> Derivations)
            let derivations = LuaEngine::load_file(&paths)?;

            // Phase 2 & 3: Build and Commit
            let switcher = Switcher::new(&paths);
            switcher.apply(&derivations, force)?;
        }
        Commands::Info => {
            let paths = paths::AppPaths::resolve(cli.config.clone());
            let _log_guard = logging::setup(cli.verbose, &paths);
            inspector::inspect(&paths)?;
        }
    }

    Ok(())
}
