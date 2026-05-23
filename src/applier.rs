//! Phase 3: Commit.
//!
//! This module is responsible for applying the computed derivations to the
//! actual filesystem. It handles atomicity, privilege elevation (sudo/doas),
//! permission management, and garbage collection of orphaned files.

use crate::builder::Builder;
use crate::model::{Derivation, DerivationKind};
use crate::state::State;
use crate::utils::hash_content;
use anyhow::{Context, Result, anyhow};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Synchronizes the desired configuration state with the filesystem.
///
/// The `Applier` reads the previous state from a local database (`state.json`),
/// compares it with the new derivations, writes the necessary files (using
/// temporary files for atomicity), manages symbolic links, and removes
/// files that are no longer tracked.
pub struct Applier {
    /// Path to the JSON file storing the state of managed configurations.
    state_path: PathBuf,
}

#[derive(Debug, PartialEq)]
enum ChangeKind {
    Created,
    Updated,
    None,
}

impl Applier {
    /// Creates a new `Applier` instance.
    pub fn new(state_path: PathBuf) -> Self {
        Self { state_path }
    }

    /// Detects the available privilege elevation tool.
    ///
    /// Currently supports `sudo` and `doas`. Returns `None` if neither is found.
    fn get_elevation_tool(&self) -> Option<&'static str> {
        if which::which("sudo").is_ok() {
            Some("sudo")
        } else if which::which("doas").is_ok() {
            Some("doas")
        } else {
            None
        }
    }

    /// Applies a list of derivations to the system.
    pub fn apply(
        &self,
        derivations: &[Derivation],
        global_force: bool,
    ) -> Result<()> {
        println!(
            "{} {}",
            style("❄").blue(),
            style("Applying configuration").bold()
        );

        let current_state = State::load(&self.state_path)?;
        let mut new_state = State::default();
        let mut seen_targets = HashSet::new();

        let pb = ProgressBar::new(derivations.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{prefix:.bold} [{bar:40.blue/white}] {pos}/{len} {msg}",
                )?
                .progress_chars("=> "),
        );
        pb.set_prefix("  Apply");

        let mut created = 0;
        let mut updated = 0;
        let mut skipped = 0;

        for der in derivations {
            let target = &der.meta.target;
            pb.set_message(format!("processing {}", der.meta.name));

            if !seen_targets.insert(target.clone()) {
                anyhow::bail!("Duplicate target path: {:?}", target);
            }

            let is_forced = global_force || der.meta.force.unwrap_or(false);

            match &der.kind {
                DerivationKind::Symlink { source_path } => {
                    match self.apply_symlink(
                        target,
                        source_path,
                        &der.meta,
                        is_forced,
                    )? {
                        ChangeKind::Created => created += 1,
                        ChangeKind::Updated => updated += 1,
                        ChangeKind::None => skipped += 1,
                    }
                    new_state.managed_files.insert(
                        target.clone(),
                        format!("symlink:{}", source_path.display()),
                    );
                }
                _ => {
                    let content = Builder::build(der)?;
                    let hash = hash_content(&content);

                    let exists_on_disk = target.exists();
                    let hash_changed =
                        current_state.managed_files.get(target) != Some(&hash);

                    if is_forced || hash_changed || !exists_on_disk {
                        self.write_file(target, &content, &der.meta)?;
                        if exists_on_disk {
                            updated += 1;
                        } else {
                            created += 1;
                        }
                    } else {
                        debug!("Skipping unchanged file: {:?}", target);
                        skipped += 1;
                    }

                    new_state.managed_files.insert(target.clone(), hash);
                }
            }
            pb.inc(1);
        }
        pb.finish_and_clear();

        let removed = self.garbage_collect(&current_state, &seen_targets)?;

        new_state.save(&self.state_path)?;

        println!(
            "  {} Finished: {} created, {} updated, {} skipped, {} removed",
            style("✓").green(),
            style(created).green(),
            style(updated).cyan(),
            style(skipped).dim(),
            style(removed).yellow()
        );

        Ok(())
    }

    /// Performs standalone garbage collection.
    ///
    /// It identifies files that are present in the state database but missing
    /// from the provided list of derivations, removes them from the filesystem,
    /// and updates the state database.
    pub fn gc(&self, derivations: &[Derivation]) -> Result<()> {
        println!(
            "{} {}",
            style("🧹").yellow(),
            style("Running garbage collection").bold()
        );

        let current_state = State::load(&self.state_path)?;
        let mut seen_targets = HashSet::new();

        for der in derivations {
            seen_targets.insert(der.meta.target.clone());
        }

        // Remove orphaned files from disk
        let removed = self.garbage_collect(&current_state, &seen_targets)?;

        // Create a new state containing only the files that were NOT orphans
        let mut new_state = State::default();
        for (path, hash) in current_state.managed_files {
            if seen_targets.contains(&path) {
                new_state.managed_files.insert(path, hash);
            }
        }

        new_state.save(&self.state_path)?;

        println!(
            "  {} Finished: {} orphaned files removed",
            style("✓").green(),
            style(removed).yellow()
        );

        Ok(())
    }

    /// Removes files that were managed in the previous state but are missing
    /// in the current configuration. If a standard remove fails due to permissions,
    /// it attempts to remove the file using elevated privileges.
    fn garbage_collect(
        &self,
        current_state: &State,
        seen_targets: &HashSet<PathBuf>,
    ) -> Result<usize> {
        let mut removed_count = 0;
        for path in current_state.managed_files.keys() {
            if seen_targets.contains(path) {
                continue;
            }

            warn!("Removing orphaned file: {:?}", path);
            if !path.exists() && fs::symlink_metadata(path).is_err() {
                continue;
            }

            if let Err(e) = fs::remove_file(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    self.remove_elevated(path)?;
                } else {
                    return Err(e).with_context(|| {
                        format!("Failed to remove orphan: {:?}", path)
                    });
                }
            }
            removed_count += 1;
        }
        Ok(removed_count)
    }

    /// Removes a file using the detected elevation tool (sudo/doas).
    fn remove_elevated(&self, path: &PathBuf) -> Result<()> {
        let tool = self.get_elevation_tool().ok_or_else(|| {
            anyhow!(
                "Permission denied and no elevation tool found for: {:?}",
                path
            )
        })?;

        duct::cmd!(tool, "rm", path).run().with_context(|| {
            format!("Failed to remove orphan with elevation: {:?}", path)
        })?;
        Ok(())
    }

    /// Dispatches file writing to standard or elevated handlers based on metadata.
    fn write_file(
        &self,
        path: &PathBuf,
        content: &str,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        let use_sudo = meta.sudo.unwrap_or(false)
            || meta.owner.is_some()
            || meta.group.is_some();

        debug!(
            "Writing{}: {:?}",
            if use_sudo { " (elevated)" } else { "" },
            path
        );

        let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));

        // Ensure parent exists before creating temp file in it
        if !parent.exists() {
            if use_sudo {
                let tool = self.get_elevation_tool().ok_or_else(|| {
                    anyhow!("Elevation requested but no elevation tool found")
                })?;
                duct::cmd!(tool, "mkdir", "-p", parent).run()?;
            } else {
                fs::create_dir_all(parent)?;
            }
        }

        // Determine where to create the temporary file.
        // - If standard write: create in the target directory
        //   to avoid cross-device link errors during `persist` (rename).
        // - If elevated write: create in the global OS temp dir (/tmp).
        //   We don't have write access to the target dir, and `sudo cp`
        //   handles cross-device copying perfectly fine.
        let temp_file = if use_sudo {
            tempfile::NamedTempFile::new()?
        } else {
            tempfile::Builder::new()
                .prefix(".icefield-tmp-")
                .tempfile_in(parent)?
        };

        fs::write(temp_file.path(), content)?;

        if use_sudo {
            self.write_elevated(temp_file.path(), path, meta)?;
        } else {
            self.write_standard(temp_file, path, meta)?;
        }

        Ok(())
    }

    /// Copies a temporary file to its final destination using elevated privileges.
    fn write_elevated(
        &self,
        temp_path: &std::path::Path,
        dest_path: &PathBuf,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        let tool = self.get_elevation_tool().ok_or_else(|| {
            anyhow!("Elevation requested but no elevation tool found")
        })?;

        duct::cmd!(tool, "cp", temp_path, dest_path).run()?;
        self.apply_ownership_elevated(tool, dest_path, meta)?;
        self.apply_permissions_elevated(tool, dest_path, meta)?;

        Ok(())
    }

    /// Applies ownership changes using an elevation tool.
    fn apply_ownership_elevated(
        &self,
        tool: &str,
        path: &PathBuf,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        if let Some(owner) = &meta.owner {
            let group = meta.group.as_deref().unwrap_or("");
            let spec = if group.is_empty() {
                owner.clone()
            } else {
                format!("{}:{}", owner, group)
            };
            duct::cmd!(tool, "chown", spec, path).run()?;
        } else if let Some(group) = &meta.group {
            duct::cmd!(tool, "chgrp", group, path).run()?;
        }
        Ok(())
    }

    /// Applies permissions (mode and executable flag) using an elevation tool.
    fn apply_permissions_elevated(
        &self,
        tool: &str,
        path: &PathBuf,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        let mode = meta.mode.unwrap_or(0o644);
        let final_mode = if meta.executable.unwrap_or(false) {
            mode | 0o111
        } else {
            mode
        };
        duct::cmd!(tool, "chmod", format!("{:o}", final_mode), path).run()?;
        Ok(())
    }

    /// Persists a temporary file to its final destination with standard privileges.
    fn write_standard(
        &self,
        temp_file: tempfile::NamedTempFile,
        dest_path: &PathBuf,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        temp_file.persist(dest_path).map_err(|e| anyhow!(e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut mode = meta.mode.unwrap_or(0o644);
            if meta.executable.unwrap_or(false) {
                mode |= 0o111;
            }
            fs::set_permissions(dest_path, fs::Permissions::from_mode(mode))?;
        }
        Ok(())
    }

    /// Dispatches symlink creation to standard or elevated handlers.
    ///
    /// Returns the kind of change performed.
    fn apply_symlink(
        &self,
        target: &PathBuf,
        source: &PathBuf,
        meta: &crate::model::CommonMeta,
        is_forced: bool,
    ) -> Result<ChangeKind> {
        let use_sudo = meta.sudo.unwrap_or(false);

        // Convert the source to an absolute path to ensure the symlink is valid
        // regardless of where it is created.
        let absolute_source =
            std::fs::canonicalize(source).unwrap_or_else(|_| source.clone());

        let exists_on_disk =
            target.exists() || fs::symlink_metadata(target).is_ok();

        if !is_forced && self.is_symlink_correct(target, &absolute_source)? {
            debug!("Symlink already correct: {:?}", target);
            return Ok(ChangeKind::None);
        }

        self.remove_target_if_exists(target, use_sudo)?;

        info!(
            "Linking{}: {:?} -> {:?}",
            if use_sudo { " (elevated)" } else { "" },
            target,
            absolute_source
        );

        if use_sudo {
            self.create_symlink_elevated(target, &absolute_source)?;
        } else {
            self.create_symlink_standard(target, &absolute_source)?;
        }

        if exists_on_disk {
            Ok(ChangeKind::Updated)
        } else {
            Ok(ChangeKind::Created)
        }
    }

    /// Checks if a symlink exists at the target path and points to the correct source.
    fn is_symlink_correct(
        &self,
        target: &PathBuf,
        source: &PathBuf,
    ) -> Result<bool> {
        if target.exists() || fs::symlink_metadata(target).is_ok() {
            let metadata = fs::symlink_metadata(target)?;
            if metadata.file_type().is_symlink()
                && &fs::read_link(target)? == source
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Removes an existing file or symlink at the target path before creating a new link.
    /// Uses elevated privileges if requested.
    fn remove_target_if_exists(
        &self,
        target: &PathBuf,
        use_sudo: bool,
    ) -> Result<()> {
        if target.exists() || fs::symlink_metadata(target).is_ok() {
            if use_sudo {
                self.remove_elevated(target)?;
            } else {
                fs::remove_file(target)?;
            }
        }
        Ok(())
    }

    /// Creates a symlink using an elevation tool, ensuring the parent directory exists.
    fn create_symlink_elevated(
        &self,
        target: &PathBuf,
        source: &PathBuf,
    ) -> Result<()> {
        let tool = self
            .get_elevation_tool()
            .ok_or_else(|| anyhow!("No elevation tool found"))?;
        if let Some(parent) = target.parent().filter(|p| !p.exists()) {
            duct::cmd!(tool, "mkdir", "-p", parent).run()?;
        }
        duct::cmd!(tool, "ln", "-sf", source, target).run()?;
        Ok(())
    }

    /// Creates a symlink with standard privileges, ensuring the parent directory exists.
    fn create_symlink_standard(
        &self,
        target: &PathBuf,
        source: &PathBuf,
    ) -> Result<()> {
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
            force: None,
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

        applier.apply(&derivations, false)?;

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
        applier.apply(&[], false)?;

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

        applier.apply(&derivations, false)?;

        assert!(fs::symlink_metadata(&target_path)?.file_type().is_symlink());
        assert_eq!(fs::read_link(&target_path)?, source_path);
        Ok(())
    }
}
