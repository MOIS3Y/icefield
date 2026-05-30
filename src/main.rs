//! Icefield: A declarative dotfile manager powered by Rust and Lua.
//!
//! This crate provides a CLI tool that allows users to define their system
//! configuration in Lua and apply it atomically. It follows a two-phase
//! approach:
//! 1. **Compute & Render**: Execute Lua to generate fully rendered derivations.
//! 2. **Commit**: Synchronize the built content with the filesystem.

mod cleaner;
mod cli;
mod crypto;
mod inspector;
mod logging;
mod lua;
mod model;
mod paths;
mod state;
mod store;
mod switcher;

use clap::Parser;
use cleaner::Cleaner;
use cli::{Cli, Commands};
use console::style;
use lua::engine::LuaEngine;
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
            let mut registry = lua::registry::ApiRegistry::new();
            lua::register(&lua, &paths, &mut registry)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("{}", registry.generate_stubs());
            return Ok(());
        }
        Commands::Switch {
            dry_run,
            force,
            backup,
        } => {
            let paths = paths::AppPaths::resolve(cli.config.clone());
            let _log_guard = logging::setup(cli.verbose, &paths);

            if dry_run {
                println!(
                    "{} {}",
                    style("?").blue(),
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

            if backup {
                println!(
                    "{} {}",
                    style("!").yellow(),
                    style("Backup mode enabled (CLI override). Unmanaged files will be preserved.")
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

            // Phase 2: Commit
            let switcher = Switcher::new(&paths);
            switcher.apply(&derivations, force, backup)?;
        }
        Commands::Info => {
            let paths = paths::AppPaths::resolve(cli.config.clone());
            let _log_guard = logging::setup(cli.verbose, &paths);
            inspector::inspect(&paths)?;
        }
        Commands::Clean { target, dry_run } => {
            let paths = paths::AppPaths::resolve(cli.config.clone());
            let _log_guard = logging::setup(cli.verbose, &paths);

            if dry_run {
                println!(
                    "{} {}",
                    style("?").blue(),
                    style("Dry run mode: No files will be deleted.").dim()
                );
            }

            let cleaner = Cleaner::new(&paths, dry_run);
            cleaner.execute(&target)?;
        }
    }

    Ok(())
}
