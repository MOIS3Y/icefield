use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about = "Declarative dotfiles manager", long_about = None)]
pub struct Cli {
    /// Path to the init.lua configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Path to the state.json file
    #[arg(short, long, value_name = "FILE")]
    pub state: Option<PathBuf>,

    /// Increase logging verbosity (-v, -vv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Apply the configuration to the system
    Apply {
        /// Dry run: show what would be done without making changes
        #[arg(short, long)]
        dry_run: bool,

        /// Force overwrite of all files, ignoring the state cache
        #[arg(short, long)]
        force: bool,
    },
    /// Clean up orphaned files (Garbage Collection)
    Gc,
    /// Show current state of managed files
    Status,
}
