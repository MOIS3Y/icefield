use directories::ProjectDirs;
use std::path::PathBuf;

/// Returns the default configuration directory and state file path.
///
/// Uses the `directories` crate to determine OS-specific paths:
/// - Config: `~/.config/icefield/init.lua` (Linux)
/// - State: `~/.cache/icefield/state.json` (Linux)
pub fn get_default_paths() -> (PathBuf, PathBuf) {
    let project_dirs = ProjectDirs::from("com", "stepan", "icefield")
        .expect("Failed to determine project directories");

    let config_dir = project_dirs.config_dir().to_path_buf();
    let state_file = project_dirs.cache_dir().join("state.json");

    (config_dir, state_file)
}
