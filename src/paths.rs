//! Path resolution and project directory management.
//!
//! This module provides the `AppPaths` structure, which determines standard
//! OS-specific directories for configuration, data, state, and cache files,
//! following platform conventions (XDG on Linux, Apple guidelines on macOS).
//!
//! It supports overrides via environment variables:
//! - `ICEFIELD_CONFIG_DIR`
//! - `ICEFIELD_DATA_DIR`
//! - `ICEFIELD_CACHE_DIR`
//! - `ICEFIELD_STATE_DIR`

use directories::ProjectDirs;
use std::env;
use std::path::{Path, PathBuf};

/// Standard file names used by the application.
pub const CONFIG_FILE: &str = "init.lua";
pub const STATE_FILE: &str = "state.json";
pub const LOG_FILE: &str = "icefield.log";

/// Manages all application-specific paths.
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// Base directory for configuration (for Lua module resolution).
    pub config_dir: PathBuf,
    /// Path to the entry point configuration file (init.lua).
    pub config_file: PathBuf,
    /// Directory for persistent data (e.g., ~/.local/share/icefield).
    pub data_dir: PathBuf,
    /// Directory for transient state (e.g., ~/.local/state/icefield).
    pub state_dir: PathBuf,
    /// Directory for cache (e.g., ~/.cache/icefield).
    pub cache_dir: PathBuf,
}

impl AppPaths {
    /// Resolves application paths based on environment variables and defaults.
    ///
    /// Hierarchy:
    /// 1. Environment variables (`ICEFIELD_CONFIG_DIR`, etc.)
    /// 2. CLI override for config (if provided)
    /// 3. OS-specific defaults (XDG)
    pub fn resolve(config_override: Option<PathBuf>) -> Self {
        let project = ProjectDirs::from("org", "MOIS3Y", "icefield")
            .expect("Failed to determine project directories");

        // 1. Resolve Config Path
        // Priority: CLI flag > Env Var > XDG default
        let config_input = config_override
            .or_else(|| env::var_os("ICEFIELD_CONFIG_DIR").map(PathBuf::from))
            .unwrap_or_else(|| project.config_dir().to_path_buf());

        // Split config_input into directory and file
        let (config_dir, config_file) = if config_input.is_file() {
            (
                config_input
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf(),
                config_input,
            )
        } else if config_input.extension().and_then(|s| s.to_str())
            == Some("lua")
        {
            // Even if it doesn't exist yet, if it ends in .lua, treat as file
            (
                config_input
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf(),
                config_input,
            )
        } else {
            // Treat as directory
            (config_input.clone(), config_input.join(CONFIG_FILE))
        };

        // 2. Resolve Data Dir (Store, State DB)
        // Priority: Env Var > XDG default
        let data_dir = env::var_os("ICEFIELD_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| project.data_dir().to_path_buf());

        // 3. Resolve State Dir (Logs)
        // Priority: Env Var > XDG default
        let state_dir = env::var_os("ICEFIELD_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                // ProjectDirs doesn't have state_dir, so we follow XDG or fallback
                env::var_os("XDG_STATE_HOME")
                    .map(|s| PathBuf::from(s).join("icefield"))
                    .unwrap_or_else(|| project.data_dir().to_path_buf())
            });

        // 4. Resolve Cache Dir
        let cache_dir = env::var_os("ICEFIELD_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| project.cache_dir().to_path_buf());

        Self {
            config_dir,
            config_file,
            data_dir,
            state_dir,
            cache_dir,
        }
    }

    /// Returns the path to the state database file (state.json).
    pub fn state_file(&self) -> PathBuf {
        self.data_dir.join(STATE_FILE)
    }

    /// Returns the directory for remote artifacts (store).
    pub fn store_dir(&self) -> PathBuf {
        self.data_dir.join("store")
    }

    /// Returns the directory for log files.
    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }
}

/// Ensures that a directory exists, creating it if necessary.
pub fn ensure_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}
