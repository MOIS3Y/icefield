use directories::ProjectDirs;
use std::path::PathBuf;

/// Standard file names used by the application.
pub const CONFIG_FILE: &str = "init.lua";
pub const STATE_FILE: &str = "state.json";
pub const LOG_FILE: &str = "icefield.log";

/// Returns the default configuration directory, state file path, and cache directory.
///
/// Uses the `directories` crate to determine OS-specific paths:
/// - Config: `~/.config/icefield/init.lua` (Linux)
/// - State: `~/.cache/icefield/state.json` (Linux)
/// - Cache: `~/.cache/icefield` (Linux)
pub fn get_default_paths() -> (PathBuf, PathBuf, PathBuf) {
    let project_dirs = ProjectDirs::from("org", "MOIS3Y", "icefield")
        .expect("Failed to determine project directories");

    let config_dir = project_dirs.config_dir().to_path_buf();
    let cache_dir = project_dirs.cache_dir().to_path_buf();
    let state_file = cache_dir.join(STATE_FILE);

    (config_dir, state_file, cache_dir)
}
