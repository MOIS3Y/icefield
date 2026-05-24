//! State management and persistence.
//!
//! This module handles the `state.json` database, which tracks the files
//! managed by Icefield and their integrity hashes. This state is crucial for
//! performing incremental updates and safe garbage collection.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Metadata for a single managed file tracked in the state database.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct ManagedFileState {
    /// The descriptive name of the derivation that produced this file.
    pub name: String,
    /// The SHA-256 hash of the file content, or a special `symlink:` prefix.
    pub hash: String,
}

/// Represents the persistent state of managed dotfiles.
///
/// Tracks which files are managed by the tool, their derivation names,
/// and content hashes to perform incremental updates and garbage collection.
/// Includes an extensible cache for future features like remote fetching.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct State {
    /// Schema version for future state migrations.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Mapping of target file paths to their metadata.
    /// Uses BTreeMap to ensure deterministic, sorted JSON output.
    pub managed_files: BTreeMap<PathBuf, ManagedFileState>,
    /// Extensible cache for storing intermediate calculations (e.g., color palettes).
    #[serde(default)]
    pub cache: serde_json::Value,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: default_version(),
            managed_files: BTreeMap::new(),
            cache: serde_json::Value::Null,
        }
    }
}

/// Returns the current default schema version for `state.json`.
fn default_version() -> u32 {
    1
}

impl State {
    /// Adds a managed file record to the state.
    pub fn add_file(&mut self, target: PathBuf, name: String, hash: String) {
        self.managed_files
            .insert(target, ManagedFileState { name, hash });
    }

    /// Loads the state from a JSON file.
    ///
    /// If the file does not exist, returns a default empty state.
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(State::default());
        }
        let content = fs::read_to_string(path)?;
        let state = serde_json::from_str(&content)?;
        Ok(state)
    }

    /// Saves the current state to a JSON file.
    ///
    /// Automatically creates parent directories if they don't exist.
    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load_state() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let state_path = dir.path().join("state.json");

        let mut state = State::default();
        state.add_file(
            PathBuf::from("config/test.conf"),
            "test-config".to_string(),
            "hash123".to_string(),
        );

        // Save
        state.save(&state_path)?;
        assert!(state_path.exists());

        // Load
        let loaded_state = State::load(&state_path)?;
        assert_eq!(state, loaded_state);

        Ok(())
    }

    #[test]
    fn test_load_non_existent_state() -> anyhow::Result<()> {
        let path = PathBuf::from("path/to/nothing/state.json");
        let state = State::load(&path)?;
        assert!(state.managed_files.is_empty());
        Ok(())
    }
}
