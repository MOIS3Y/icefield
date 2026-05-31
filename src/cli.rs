//! Command-line interface definition.
//!
//! This module defines the CLI structure using `clap`, including global
//! flags and subcommands for applying configurations, garbage collection,
//! and status inspection.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about = "Declarative dotfiles manager", long_about = None)]
pub struct Cli {
    /// Path to the init.lua configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Increase logging verbosity (-v, -vv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Switch to the provided configuration
    Switch {
        /// Dry run: show what would be done without making changes
        #[arg(short, long)]
        dry_run: bool,

        /// Force overwrite of all files, ignoring the state cache
        #[arg(short, long)]
        force: bool,

        /// Enable backups for unmanaged files (overrides global config)
        #[arg(short, long)]
        backup: bool,
    },
    /// Clean up Icefield state, store, and logs
    Clean {
        #[command(subcommand)]
        target: CleanTarget,

        /// Preview what would be deleted without actually removing anything
        #[arg(short, long, global = true)]
        dry_run: bool,
    },
    /// Show information about managed files and current state
    Info,
    /// Generate EmmyLua stubs for IDE autocompletion
    Stubs,
    /// Calculate the content-addressable fingerprint for a directory or remote resource
    Fingerprint {
        /// The path to the directory or a remote URL (e.g. github:user/repo)
        target: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CleanTarget {
    /// Smart GC: Removes artifacts from the store that are not in the current config
    Store,
    /// Smart Uninstall: Removes managed files from the filesystem and clears state
    State,
    /// Truncates the central log file
    Logs,
    /// Complete reset: cleans state, wipes store, and clears logs
    All,
}
