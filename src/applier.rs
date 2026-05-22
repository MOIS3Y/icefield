use crate::builder::Builder;
use crate::model::{Derivation, DerivationKind};
use crate::state::State;
use crate::utils::hash_content;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// The `Applier` is responsible for "Phase 3: Commit".
///
/// It synchronizes the desired state (calculated in Phase 1 and built in
/// Phase 2) with the actual state of the filesystem. It also handles
/// garbage collection by removing files that are no longer managed.
pub struct Applier {
    state_path: PathBuf,
}

impl Applier {
    /// Creates a new `Applier` with the specified state database path.
    pub fn new(state_path: PathBuf) -> Self {
        Self { state_path }
    }

    /// Applies a list of derivations to the system.
    ///
    /// This method:
    /// 1. Loads the current state from disk.
    /// 2. Iterates through derivations, building and writing them if changed.
    /// 3. Detects and removes "orphaned" files (files in state but not in config).
    /// 4. Saves the new state back to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if any file operation fails or if duplicate target
    /// paths are detected in the configuration.
    pub fn apply(&self, derivations: &[Derivation]) -> Result<()> {
        info!("Applying configuration...");
        let current_state = State::load(&self.state_path)?;
        let mut new_state = State::default();
        let mut seen_targets = HashSet::new();

        for der in derivations {
            let target = &der.meta.target;

            if seen_targets.contains(target) {
                anyhow::bail!("Duplicate target path: {:?}", target);
            }
            seen_targets.insert(target.clone());

            match &der.kind {
                DerivationKind::Symlink { source_path } => {
                    self.apply_symlink(target, source_path)?;
                    new_state.managed_files.insert(
                        target.clone(),
                        format!("symlink:{}", source_path.display()),
                    );
                }
                _ => {
                    let content = Builder::build(der)?;
                    let hash = hash_content(&content);

                    if current_state.managed_files.get(target) != Some(&hash)
                        || !target.exists()
                    {
                        self.write_file(target, &content, &der.meta)?;
                    } else {
                        debug!("Skipping unchanged file: {:?}", target);
                    }

                    new_state.managed_files.insert(target.clone(), hash);
                }
            }
        }

        // Garbage collection: remove files that were in the state but are no
        // longer in the current configuration.
        for path in current_state.managed_files.keys() {
            if !seen_targets.contains(path) {
                warn!("Removing orphaned file: {:?}", path);
                if path.exists() {
                    fs::remove_file(path).with_context(|| {
                        format!("Failed to remove orphan: {:?}", path)
                    })?;
                }
            }
        }

        new_state.save(&self.state_path)?;
        info!("Apply finished successfully.");
        Ok(())
    }

    /// Writes content to a file and ensures parent directories exist.
    ///
    /// On Unix systems, it also sets the file mode (permissions).
    fn write_file(
        &self,
        path: &PathBuf,
        content: &str,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        info!("Writing: {:?}", path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut mode = meta.mode.unwrap_or(0o644);
            if meta.executable.unwrap_or(false) {
                mode |= 0o111;
            }
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        }

        Ok(())
    }

    /// Manages a symbolic link.
    ///
    /// If a file or link already exists at the target path, it will be
    /// removed if it points to the wrong location or if it's a regular file.
    fn apply_symlink(&self, target: &PathBuf, source: &PathBuf) -> Result<()> {
        if target.exists() || fs::symlink_metadata(target).is_ok() {
            let metadata = fs::symlink_metadata(target)?;
            if metadata.file_type().is_symlink() {
                if &fs::read_link(target)? == source {
                    debug!("Symlink already correct: {:?}", target);
                    return Ok(());
                }
                fs::remove_file(target)?;
            } else {
                warn!("Replacing regular file with symlink: {:?}", target);
                fs::remove_file(target)?;
            }
        }

        info!("Linking: {:?} -> {:?}", target, source);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(source, target)?;
        #[cfg(not(unix))]
        return Err(anyhow::anyhow!("Symlinks are only supported on Unix"));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CommonMeta;
    use serde_json::json;
    use tempfile::tempdir;

    fn mock_meta(target: PathBuf) -> CommonMeta {
        CommonMeta {
            name: "test".to_string(),
            target,
            sudo: None,
            owner: None,
            group: None,
            mode: None,
            executable: None,
        }
    }

    #[test]
    fn test_apply_new_file() -> Result<()> {
        let dir = tempdir()?;
        let state_path = dir.path().join("state.json");
        let target_path = dir.path().join("test.txt");

        let applier = Applier::new(state_path.clone());
        let derivations = vec![Derivation {
            meta: mock_meta(target_path.clone()),
            kind: DerivationKind::Toml {
                source: json!({ "foo": "bar" }),
            },
        }];

        applier.apply(&derivations)?;

        assert!(target_path.exists());
        assert_eq!(fs::read_to_string(&target_path)?, "foo = \"bar\"\n");

        let state = State::load(&state_path)?;
        assert!(state.managed_files.contains_key(&target_path));
        Ok(())
    }

    #[test]
    fn test_apply_garbage_collection() -> Result<()> {
        let dir = tempdir()?;
        let state_path = dir.path().join("state.json");
        let target_path = dir.path().join("orphan.txt");

        // Pre-create an "orphaned" file and a state that tracks it
        fs::write(&target_path, "orphan")?;
        let mut initial_state = State::default();
        initial_state
            .managed_files
            .insert(target_path.clone(), "old-hash".to_string());
        initial_state.save(&state_path)?;

        let applier = Applier::new(state_path);
        // Apply an empty config
        applier.apply(&[])?;

        assert!(!target_path.exists());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_apply_symlink() -> Result<()> {
        let dir = tempdir()?;
        let state_path = dir.path().join("state.json");
        let target_path = dir.path().join("link");
        let source_path = dir.path().join("source.txt");
        fs::write(&source_path, "content")?;

        let applier = Applier::new(state_path);
        let derivations = vec![Derivation {
            meta: mock_meta(target_path.clone()),
            kind: DerivationKind::Symlink {
                source_path: source_path.clone(),
            },
        }];

        applier.apply(&derivations)?;

        assert!(fs::symlink_metadata(&target_path)?.file_type().is_symlink());
        assert_eq!(fs::read_link(&target_path)?, source_path);
        Ok(())
    }
}
