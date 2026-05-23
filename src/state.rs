use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Represents the persistent state of managed dotfiles.
///
/// Tracks which files are managed by the tool and their content hashes
/// to perform incremental updates and garbage collection.
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct State {
    /// Mapping of target file paths to their SHA-256 hashes or symlink info.
    /// Uses BTreeMap to ensure deterministic, sorted JSON output.
    pub managed_files: BTreeMap<PathBuf, String>,
}

impl State {
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
        state
            .managed_files
            .insert(PathBuf::from("/tmp/test.conf"), "hash123".to_string());

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
        let path = PathBuf::from("/non/existent/path/state.json");
        let state = State::load(&path)?;
        assert!(state.managed_files.is_empty());
        Ok(())
    }
}
