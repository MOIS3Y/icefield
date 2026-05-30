//! Phase 2: Commit.
//!
//! This module is responsible for applying the computed derivations to the
//! actual filesystem. It handles atomicity, privilege elevation (sudo/doas),
//! permission management, and garbage collection of orphaned files.

use crate::crypto::{hash_content, hash_file};
use crate::model::{Derivation, DerivationKind};
use crate::paths;
use crate::state::State;
use anyhow::{Context, Result, anyhow};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Synchronizes the desired configuration state with the filesystem.
///
/// The `Switcher` reads the previous state from a local database (`state.json`),
/// compares it with the new derivations, writes the necessary files (using
/// temporary files for atomicity), manages symbolic links, and removes
/// files that are no longer tracked.
pub struct Switcher {
    /// Resolved application paths.
    paths: paths::AppPaths,
}

#[derive(Debug, PartialEq)]
enum ChangeKind {
    Created,
    Updated,
    None,
}

impl Switcher {
    /// Creates a new `Switcher` instance.
    pub fn new(paths: &paths::AppPaths) -> Self {
        Self {
            paths: paths.clone(),
        }
    }

    /// Pre-flight check: detects unmanaged files at target paths.
    /// Either backs them up or aborts execution if backups are not enabled.
    fn handle_collisions(
        &self,
        derivations: &[Derivation],
        state: &State,
        cli_backup: bool,
    ) -> Result<()> {
        let mut fatal_collisions = Vec::new();
        let mut to_backup = Vec::new();

        for der in derivations {
            let target = &der.meta.dst;

            // Check if file exists and is NOT managed by us
            if (target.exists() || fs::symlink_metadata(target).is_ok())
                && !state.managed_files.contains_key(target)
            {
                if cli_backup {
                    to_backup.push(target);
                } else {
                    fatal_collisions.push(target);
                }
            }
        }

        if !fatal_collisions.is_empty() {
            println!(
                "\n{} {}",
                style("[ERROR]").red().bold(),
                style("Collision detected!").bold()
            );
            println!(
                "The following files exist and are not managed by Icefield:"
            );
            for path in &fatal_collisions {
                println!("  - {}", style(path.display()).yellow());
            }
            println!(
                "Please remove them manually or run with {} to move them aside.\n",
                style("--backup").cyan()
            );
            anyhow::bail!("Pre-flight checks failed due to collisions.");
        }

        for path in to_backup {
            let backup_path = PathBuf::from(format!(
                "{}{}",
                path.display(),
                ".icefield-bak"
            ));
            info!(
                "Backing up unmanaged file: {:?} -> {:?}",
                path, backup_path
            );
            println!(
                "  {} Backed up unmanaged file {}",
                style("b").cyan(),
                path.display()
            );

            if let Err(e) = fs::rename(path, &backup_path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    if let Some(tool) = self.get_elevation_tool() {
                        duct::cmd!(tool, "mv", path, &backup_path)
                            .run()
                            .with_context(|| {
                                format!(
                                    "Failed to backup elevated file: {:?}",
                                    path
                                )
                            })?;
                        continue;
                    }
                }
                return Err(e)
                    .context(format!("Failed to backup file {:?}", path));
            }
        }

        Ok(())
    }

    /// Determines if a derivation requires privilege elevation.
    ///
    /// Elevation is required if `sudo` is true, or if a specific `owner` or
    /// `group` is requested.
    fn needs_elevation(&self, meta: &crate::model::CommonMeta) -> bool {
        meta.sudo.unwrap_or(false)
            || meta.owner.is_some()
            || meta.group.is_some()
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
        cli_backup: bool,
    ) -> Result<()> {
        println!(
            "{} {}",
            style("❄").blue(),
            style("Applying configuration").bold()
        );

        let state_file = self.paths.state_file();
        let current_state = State::load(&state_file)?;

        // Pre-flight: check for collisions before any writes.
        self.handle_collisions(derivations, &current_state, cli_backup)?;

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
            let target = &der.meta.dst;
            pb.set_message(format!("processing {}", der.meta.name));

            if !seen_targets.insert(target.clone()) {
                anyhow::bail!("Duplicate target path: {:?}", target);
            }

            let is_forced = global_force || der.meta.force.unwrap_or(false);

            match &der.kind {
                DerivationKind::Symlink { src } => {
                    match self
                        .apply_symlink(target, src, &der.meta, is_forced)?
                    {
                        ChangeKind::Created => created += 1,
                        ChangeKind::Updated => updated += 1,
                        ChangeKind::None => skipped += 1,
                    }
                    new_state.add_file(
                        target.clone(),
                        der.meta.name.clone(),
                        format!("symlink:{}", src.display()),
                    );
                }
                DerivationKind::Copy { src } => {
                    let hash = hash_file(src)?;
                    let exists_on_disk = target.exists();
                    let hash_changed = current_state
                        .managed_files
                        .get(target)
                        .map(|s| s.hash.as_str())
                        != Some(&hash);

                    if is_forced || hash_changed || !exists_on_disk {
                        self.copy_file(src, target, &der.meta)?;
                        if exists_on_disk {
                            updated += 1;
                        } else {
                            created += 1;
                        }
                    } else {
                        debug!("Skipping unchanged file: {:?}", target);
                        skipped += 1;
                    }
                    new_state.add_file(
                        target.clone(),
                        der.meta.name.clone(),
                        hash,
                    );
                }
                DerivationKind::Text { src } => {
                    let content = src;
                    let hash = hash_content(content);
                    let exists_on_disk = target.exists();
                    let hash_changed = current_state
                        .managed_files
                        .get(target)
                        .map(|s| s.hash.as_str())
                        != Some(&hash);

                    if is_forced || hash_changed || !exists_on_disk {
                        self.write_text_file(target, content, &der.meta)?;
                        if exists_on_disk {
                            updated += 1;
                        } else {
                            created += 1;
                        }
                    } else {
                        debug!("Skipping unchanged file: {:?}", target);
                        skipped += 1;
                    }
                    new_state.add_file(
                        target.clone(),
                        der.meta.name.clone(),
                        hash,
                    );
                }
            }
            pb.inc(1);
        }

        pb.finish_and_clear();

        // Perform GC: remove files that were in the old state but not in the new one.
        self.garbage_collect(&current_state, &new_state)?;

        new_state.save(&state_file)?;

        println!(
            "  {} Finished: {} created, {} updated, {} skipped, {} removed",
            style("✓").green(),
            style(created).green(),
            style(updated).cyan(),
            style(skipped).dim(),
            style(
                new_state
                    .managed_files
                    .len()
                    .saturating_sub(created + updated + skipped)
            )
            .yellow()
        );

        Ok(())
    }

    /// Removes files that are no longer part of the managed configuration.
    fn garbage_collect(
        &self,
        old_state: &State,
        new_state: &State,
    ) -> Result<()> {
        let mut removed = 0;
        for (path, _) in &old_state.managed_files {
            if !new_state.managed_files.contains_key(path) {
                if !path.exists() && fs::symlink_metadata(path).is_err() {
                    continue;
                }
                info!("Garbage collecting orphaned file: {:?}", path);
                fs::remove_file(path).with_context(|| {
                    format!("Failed to garbage collect file: {:?}", path)
                })?;
                removed += 1;
            }
        }
        if removed > 0 {
            debug!("Removed {} orphaned files", removed);
        }
        Ok(())
    }

    /// Atomically writes content to a file.
    ///
    /// This method ensures the parent directory exists and uses temporary
    /// files to prevent partial writes. If the target requires special
    /// permissions, it delegates to the elevated write method.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent directory cannot be created or if
    /// the file write fails.
    fn write_text_file(
        &self,
        target: &Path,
        content: &str,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        let parent = target.parent().ok_or_else(|| {
            anyhow!("Target path has no parent directory: {:?}", target)
        })?;
        paths::ensure_dir(parent)?;

        if self.needs_elevation(meta) {
            self.write_text_file_elevated(target, content, meta)
        } else {
            self.write_text_file_atomic(target, content, meta)
        }
    }

    /// Performs a standard atomic write using a temporary file.
    fn write_text_file_atomic(
        &self,
        target: &Path,
        content: &str,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        let parent = target.parent().unwrap();
        let temp_file = tempfile::Builder::new()
            .prefix(".icefield-tmp")
            .tempfile_in(parent)
            .context("Failed to create temporary file")?;

        fs::write(temp_file.path(), content)
            .context("Failed to write to temporary file")?;

        self.apply_metadata(temp_file.path(), meta)?;

        temp_file.persist(target).map_err(|e| {
            anyhow!("Failed to persist file {:?}: {}", target, e)
        })?;

        Ok(())
    }

    /// Performs an elevated write using the system's temporary directory.
    fn write_text_file_elevated(
        &self,
        target: &Path,
        content: &str,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let temp_file = tempfile::Builder::new()
            .prefix("icefield-elevated")
            .tempfile_in(temp_dir)
            .context("Failed to create elevated temporary file")?;

        fs::write(temp_file.path(), content)
            .context("Failed to write to elevated temporary file")?;

        let tool = self.get_elevation_tool().ok_or_else(|| {
            anyhow!(
                "Privilege elevation required but no tool found (sudo/doas)"
            )
        })?;

        duct::cmd!(tool, "mv", temp_file.path(), target)
            .run()
            .with_context(|| {
                format!("Failed to move elevated file to {:?}", target)
            })?;

        self.apply_metadata_elevated(target, meta, tool)?;

        Ok(())
    }

    /// Copies a physical file to the target destination.
    ///
    /// # Errors
    ///
    /// Returns an error if the source cannot be read or target cannot be written.
    fn copy_file(
        &self,
        src: &Path,
        target: &Path,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        let parent = target.parent().ok_or_else(|| {
            anyhow!("Target path has no parent directory: {:?}", target)
        })?;
        paths::ensure_dir(parent)?;

        if self.needs_elevation(meta) {
            let tool = self.get_elevation_tool().ok_or_else(|| {
                anyhow!("Privilege elevation required but no tool found (sudo/doas)")
            })?;
            duct::cmd!(tool, "cp", src, target).run().with_context(|| {
                format!("Failed to copy elevated file to {:?}", target)
            })?;
            self.apply_metadata_elevated(target, meta, tool)?;
        } else {
            fs::copy(src, target).with_context(|| {
                format!("Failed to copy file from {:?} to {:?}", src, target)
            })?;
            self.apply_metadata(target, meta)?;
        }
        Ok(())
    }

    /// Manages a symbolic link at the target path.
    ///
    /// If the path exists and is not a symlink, it will attempt to remove it
    /// if `force` is true, otherwise it will return an error.
    ///
    /// # Errors
    ///
    /// Returns an error if symlink creation fails or if a collision occurs.
    fn apply_symlink(
        &self,
        target: &Path,
        source: &Path,
        meta: &crate::model::CommonMeta,
        force: bool,
    ) -> Result<ChangeKind> {
        let parent = target.parent().ok_or_else(|| {
            anyhow!("Target path has no parent directory: {:?}", target)
        })?;
        paths::ensure_dir(parent)?;

        // Canonicalize source to make comparison reliable
        let source =
            fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());

        if target.exists() || fs::symlink_metadata(target).is_ok() {
            let is_symlink =
                fs::symlink_metadata(target)?.file_type().is_symlink();
            if is_symlink {
                let current_source = fs::read_link(target)?;
                if current_source == source && !force {
                    return Ok(ChangeKind::None);
                }
            }
            if force || is_symlink {
                if self.needs_elevation(meta) {
                    let tool = self.get_elevation_tool().ok_or_else(|| {
                        anyhow!("Privilege elevation tool required")
                    })?;
                    duct::cmd!(tool, "rm", "-rf", target).run()?;
                } else {
                    if target.is_dir() && !is_symlink {
                        fs::remove_dir_all(target)?;
                    } else {
                        fs::remove_file(target)?;
                    }
                }
            } else {
                anyhow::bail!(
                    "Path exists and is not a symlink: {:?}",
                    target
                );
            }
        }

        if self.needs_elevation(meta) {
            let tool = self
                .get_elevation_tool()
                .ok_or_else(|| anyhow!("Privilege elevation tool required"))?;
            duct::cmd!(tool, "ln", "-s", source, target).run()?;
        } else {
            #[cfg(unix)]
            std::os::unix::fs::symlink(source, target)?;
        }

        if target.exists() {
            Ok(ChangeKind::Updated)
        } else {
            Ok(ChangeKind::Created)
        }
    }

    /// Applies standard Unix metadata (mode/permissions) to a file.
    fn apply_metadata(
        &self,
        path: &Path,
        meta: &crate::model::CommonMeta,
    ) -> Result<()> {
        if let Some(mode_str) = &meta.mode {
            use std::os::unix::fs::PermissionsExt;
            let mode = u32::from_str_radix(mode_str, 8)
                .context("Invalid octal mode")?;
            fs::set_permissions(path, fs::Permissions::from_mode(mode))
                .context("Failed to set permissions")?;
        }
        Ok(())
    }

    /// Applies metadata using elevated privileges.
    fn apply_metadata_elevated(
        &self,
        path: &Path,
        meta: &crate::model::CommonMeta,
        tool: &str,
    ) -> Result<()> {
        if let Some(mode_str) = &meta.mode {
            duct::cmd!(tool, "chmod", mode_str, path).run()?;
        }
        if let Some(owner) = &meta.owner {
            duct::cmd!(tool, "chown", owner, path).run()?;
        }
        if let Some(group) = &meta.group {
            duct::cmd!(tool, "chgrp", group, path).run()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CommonMeta;
    use tempfile::tempdir;

    fn mock_paths(base: &Path) -> paths::AppPaths {
        paths::AppPaths {
            config_dir: base.join("config"),
            config_file: base.join("config").join("init.lua"),
            data_dir: base.join("data"),
            state_dir: base.join("state"),
            cache_dir: base.join("cache"),
        }
    }

    #[test]
    fn test_apply_new_file() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());
        let target_path = dir.path().join("test.txt");
        let state_path = paths.state_file();
        let derivations = vec![Derivation {
            meta: CommonMeta {
                name: "test".to_string(),
                enable: true,
                dst: target_path.clone(),
                force: None,
                sudo: None,
                owner: None,
                group: None,
                mode: None,
            },
            kind: DerivationKind::Text {
                src: "hello".to_string(),
            },
        }];

        let switcher = Switcher::new(&paths);
        switcher.apply(&derivations, false, false)?;

        assert!(target_path.exists());
        assert_eq!(fs::read_to_string(&target_path)?, "hello");

        let state = State::load(&state_path)?;
        assert!(state.managed_files.contains_key(&target_path));
        Ok(())
    }

    #[test]
    fn test_apply_garbage_collection() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());
        let target_path = dir.path().join("to_be_removed.txt");
        let state_path = paths.state_file();

        fs::create_dir_all(target_path.parent().unwrap())?;
        fs::write(&target_path, "old content")?;

        let mut initial_state = State::default();
        initial_state.add_file(
            target_path.clone(),
            "old-der".to_string(),
            "old-hash".to_string(),
        );
        paths::ensure_dir(state_path.parent().unwrap())?;
        initial_state.save(&state_path)?;

        let switcher = Switcher::new(&paths);
        // Apply an empty config
        switcher.apply(&[], false, false)?;

        assert!(!target_path.exists());
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_apply_symlink() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());
        let target_path = dir.path().join("link");
        let source = dir.path().join("source.txt");
        fs::write(&source, "source content")?;
        let derivations = vec![Derivation {
            meta: CommonMeta {
                name: "test".to_string(),
                enable: true,
                dst: target_path.clone(),
                force: None,
                sudo: None,
                owner: None,
                group: None,
                mode: None,
            },
            kind: DerivationKind::Symlink {
                src: source.clone(),
            },
        }];

        let switcher = Switcher::new(&paths);
        switcher.apply(&derivations, false, false)?;

        assert!(fs::symlink_metadata(&target_path)?.file_type().is_symlink());
        assert_eq!(fs::read_link(&target_path)?, source);
        Ok(())
    }
}
