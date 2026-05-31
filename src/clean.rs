//! Maintenance and garbage collection.
//!
//! This module implements advanced cleanup functionalities, analogous to
//! `nix-collect-garbage`. It handles removing unused artifacts from the store,
//! safely uninstalling managed files to revert the system state, and clearing logs.

use crate::cli::CleanTarget;
use crate::crypto::hash_file;
use crate::paths::AppPaths;
use crate::state::State;
use anyhow::{Context, Result};
use console::style;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Orchestrates the cleanup operations for the system.
pub struct Cleaner {
    paths: AppPaths,
    dry_run: bool,
}

impl Cleaner {
    /// Creates a new `Cleaner` instance.
    #[must_use]
    pub fn new(paths: &AppPaths, dry_run: bool) -> Self {
        Self {
            paths: paths.clone(),
            dry_run,
        }
    }

    /// Executes the specified cleanup target.
    ///
    /// # Errors
    ///
    /// Returns an error if any of the underlying cleanup operations fail.
    pub fn execute(&self, target: &CleanTarget) -> Result<()> {
        match target {
            CleanTarget::Logs => self.clean_logs()?,
            CleanTarget::State => self.clean_state()?,
            CleanTarget::Store => self.clean_store()?,
            CleanTarget::All => {
                self.clean_state()?;
                self.clean_store()?;
                self.clean_logs()?;

                if self.dry_run {
                    println!("\n{} Dry run complete.", style("★").magenta());
                } else {
                    println!("\n{} System is clean!", style("★").magenta());
                }
            }
        }
        Ok(())
    }

    /// Truncates the central log file to zero bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the log file exists but cannot be opened for truncation.
    fn clean_logs(&self) -> Result<()> {
        println!("{} Clearing logs...", style("≡").magenta());
        let log_file = self.paths.log_dir().join(crate::paths::LOG_FILE);

        if !log_file.exists() {
            println!("  {} Logs are already clean.", style("✓").green());
            return Ok(());
        }

        let size = fs::metadata(&log_file).map(|m| m.len()).unwrap_or(0);

        if self.dry_run {
            println!(
                "  Would clear log file: {} ({})",
                log_file.display(),
                format_size(size)
            );
            return Ok(());
        }

        fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&log_file)
            .with_context(|| {
                format!("Failed to truncate log file: {:?}", log_file)
            })?;

        println!(
            "  {} Cleared {} of logs.",
            style("✓").green(),
            format_size(size)
        );

        Ok(())
    }

    /// Smart Uninstall: Reads state.json and physically removes managed files.
    ///
    /// It verifies hashes before removal and skips files that were modified manually.
    ///
    /// # Errors
    ///
    /// Returns an error if the state database cannot be loaded or updated.
    fn clean_state(&self) -> Result<()> {
        println!("{} Reverting managed system state...", style("⟲").magenta());
        let state_file = self.paths.state_file();

        if !state_file.exists() {
            println!(
                "  {} No managed state found. System is clean.",
                style("✓").green()
            );
            return Ok(());
        }

        let state = State::load(&state_file)?;
        let mut removed_count = 0;
        let mut skipped_count = 0;
        let mut dirs_to_check = HashSet::new();

        for (target_path, file_state) in &state.managed_files {
            self.process_state_file(
                target_path,
                &file_state.hash,
                &mut removed_count,
                &mut skipped_count,
                &mut dirs_to_check,
            );
        }

        self.cleanup_empty_directories(dirs_to_check);
        self.finalize_state_cleanup(
            &state_file,
            removed_count,
            skipped_count,
        )?;

        Ok(())
    }

    /// Processes a single file from the state database for removal.
    fn process_state_file(
        &self,
        target_path: &Path,
        expected_hash: &str,
        removed_count: &mut usize,
        skipped_count: &mut usize,
        dirs_to_check: &mut HashSet<PathBuf>,
    ) {
        if !target_path.exists() && fs::symlink_metadata(target_path).is_err()
        {
            return;
        }

        if self.is_file_modified(target_path, expected_hash) {
            self.report_skipped_modification(target_path);
            *skipped_count += 1;
            return;
        }

        if self.dry_run {
            println!("  Would remove: {}", target_path.display());
            return;
        }

        if self.remove_managed_file(target_path) {
            *removed_count += 1;
            if let Some(parent) = target_path.parent() {
                dirs_to_check.insert(parent.to_path_buf());
            }
        } else {
            *skipped_count += 1;
        }
    }

    /// Checks if a managed file has been manually modified by comparing hashes.
    fn is_file_modified(&self, path: &Path, expected_hash: &str) -> bool {
        if let Some(expected_src) = expected_hash.strip_prefix("symlink:") {
            return match fs::read_link(path) {
                Ok(current_src) => {
                    current_src.to_string_lossy() != expected_src
                }
                Err(_) => true,
            };
        }

        if path.is_file()
            && let Ok(current_hash) = hash_file(path)
        {
            return current_hash != expected_hash;
        }
        false
    }

    /// Prints a warning that a file was skipped due to manual modification.
    fn report_skipped_modification(&self, path: &Path) {
        if self.dry_run {
            println!(
                "  {} Would skip {} (content modified manually)",
                style("!").yellow(),
                path.display()
            );
        } else {
            println!(
                "  {} Skipped {} (content modified manually)",
                style("!").yellow(),
                path.display()
            );
            warn!("Skipped uninstalling modified file: {:?}", path);
        }
    }

    /// Attempts to remove a managed file with elevation fallback.
    fn remove_managed_file(&self, path: &Path) -> bool {
        match fs::remove_file(path) {
            Ok(_) => {
                println!(
                    "  {} Removed {}",
                    style("✓").green(),
                    path.display()
                );
                info!("Uninstalled file: {:?}", path);
                true
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    self.remove_managed_file_elevated(path)
                } else {
                    println!(
                        "  {} Failed to remove {}: {}",
                        style("!").red(),
                        path.display(),
                        e
                    );
                    warn!("Failed to uninstall file {:?}: {}", path, e);
                    false
                }
            }
        }
    }

    /// Attempts to remove a file using a privilege elevation tool.
    fn remove_managed_file_elevated(&self, path: &Path) -> bool {
        if let Some(tool) = get_elevation_tool()
            && duct::cmd!(tool, "rm", path).run().is_ok()
        {
            println!(
                "  {} Removed (elevated) {}",
                style("✓").green(),
                path.display()
            );
            info!("Uninstalled file (elevated): {:?}", path);
            return true;
        }
        println!(
            "  {} Failed to remove (permission denied): {}",
            style("!").red(),
            path.display()
        );
        false
    }

    /// Recursively removes empty parent directories of deleted files.
    fn cleanup_empty_directories(&self, dirs_to_check: HashSet<PathBuf>) {
        if self.dry_run {
            return;
        }

        let mut sorted_dirs: Vec<_> = dirs_to_check.into_iter().collect();
        sorted_dirs.sort_by_key(|b| std::cmp::Reverse(b.as_os_str().len()));

        for dir in sorted_dirs {
            self.remove_empty_dirs_recursively(&dir);
        }
    }

    /// Internal recursive helper for removing empty directories.
    fn remove_empty_dirs_recursively(&self, dir: &Path) {
        if let Ok(entries) = fs::read_dir(dir)
            && entries.count() == 0
            && fs::remove_dir(dir).is_ok()
            && let Some(parent) = dir.parent()
        {
            self.remove_empty_dirs_recursively(parent);
        }
    }

    /// Resets the state database and prints the uninstall summary.
    ///
    /// # Errors
    ///
    /// Returns an error if the state database cannot be saved.
    fn finalize_state_cleanup(
        &self,
        state_file: &Path,
        removed_count: usize,
        skipped_count: usize,
    ) -> Result<()> {
        if self.dry_run {
            println!("  Would clear state database: {}", state_file.display());
            return Ok(());
        }

        let empty_state = State::default();
        empty_state.save(&state_file.to_path_buf())?;

        if removed_count == 0 && skipped_count == 0 {
            println!(
                "  {} System state was already clean.",
                style("✓").green()
            );
        }
        Ok(())
    }

    /// Smart GC: Removes artifacts from the store that are not referenced in the current state.
    ///
    /// # Errors
    ///
    /// Returns an error if the store directory cannot be read or artifacts cannot be removed.
    fn clean_store(&self) -> Result<()> {
        println!("{} Emptying store cache...", style("♻").magenta());
        let store_dir = self.paths.store_dir();

        if !store_dir.exists() {
            println!("  {} Store is already empty.", style("✓").green());
            return Ok(());
        }

        // Smart GC: Load state to find which artifacts are actually in use
        let state_file = self.paths.state_file();
        let state = if state_file.exists() {
            State::load(&state_file)?
        } else {
            State::default()
        };

        self.perform_smart_gc(&store_dir, &state)
    }

    /// Performs the Smart GC logic to remove unused artifacts.
    fn perform_smart_gc(&self, store_dir: &Path, state: &State) -> Result<()> {
        let active_store_paths =
            self.calculate_active_store_paths(store_dir, state);

        let mut freed_bytes = 0;
        let mut removed_count = 0;

        let entries = fs::read_dir(store_dir).with_context(|| {
            format!("Failed to read store directory: {:?}", store_dir)
        })?;

        for entry in entries.flatten() {
            self.process_store_entry(
                &entry,
                &active_store_paths,
                &mut freed_bytes,
                &mut removed_count,
            );
        }

        self.print_gc_summary(freed_bytes, removed_count);
        Ok(())
    }

    /// Calculates which artifact folders in the store are currently active.
    fn calculate_active_store_paths(
        &self,
        store_dir: &Path,
        state: &State,
    ) -> HashSet<std::ffi::OsString> {
        let mut active = HashSet::new();

        // 1. Get all entries in the store directory once
        let store_entries: Vec<_> = fs::read_dir(store_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .collect();

        for (target, file_state) in &state.managed_files {
            // Check symlink targets
            if let Some(rest) = file_state.hash.strip_prefix("symlink:")
                && let Ok(rel) = Path::new(rest).strip_prefix(store_dir)
                && let Some(folder) = rel.iter().next()
            {
                active.insert(folder.to_os_string());
            }

            // Check content-addressable store folders
            // A folder is active if its name starts with the hash prefix from state
            let hash_prefix = &file_state.hash;
            for entry in &store_entries {
                let name = entry.file_name();
                if let Some(name_str) = name.to_str()
                    && name_str.starts_with(hash_prefix)
                {
                    active.insert(name);
                }
            }

            // Also check if the managed file itself is inside the store
            if let Ok(rel) = target.strip_prefix(store_dir)
                && let Some(folder) = rel.iter().next()
            {
                active.insert(folder.to_os_string());
            }
        }
        active
    }

    /// Processes a single artifact folder in the store.
    fn process_store_entry(
        &self,
        entry: &fs::DirEntry,
        active_store_paths: &HashSet<std::ffi::OsString>,
        freed_bytes: &mut u64,
        removed_count: &mut usize,
    ) {
        let path = entry.path();
        let folder_name = entry.file_name();

        if active_store_paths.contains(&folder_name) {
            return;
        }

        let size = get_dir_size(&path);

        if self.dry_run {
            println!(
                "  Would remove unused artifact: {} ({})",
                folder_name.to_string_lossy(),
                format_size(size)
            );
            *freed_bytes += size;
            *removed_count += 1;
            return;
        }

        if let Err(e) = fs::remove_dir_all(&path) {
            warn!("Failed to remove artifact cache {:?}: {}", path, e);
        } else {
            info!("GC removed artifact: {:?}", path);
            *freed_bytes += size;
            *removed_count += 1;
        }
    }

    /// Prints the final summary of the Garbage Collection process.
    fn print_gc_summary(&self, freed_bytes: u64, removed_count: usize) {
        if removed_count == 0 {
            println!("  {} No unused artifacts found.", style("✓").green());
            return;
        }

        if self.dry_run {
            println!(
                "  Would free {} ({} unused artifacts)",
                format_size(freed_bytes),
                removed_count
            );
        } else {
            println!(
                "  {} Freed {} ({} unused artifacts)",
                style("✓").green(),
                format_size(freed_bytes),
                removed_count
            );
        }
    }
}

/// Recursively calculates the total size of a directory in bytes.
fn get_dir_size(path: &Path) -> u64 {
    let mut size = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_dir() {
                    size += get_dir_size(&entry.path());
                } else {
                    size += metadata.len();
                }
            }
        }
    }
    size
}

/// Formats a byte size into a human-readable string (KB/MB).
fn format_size(size: u64) -> String {
    let mb = size as f64 / 1_048_576.0;
    if mb >= 1.0 {
        format!("{:.2} MB", mb)
    } else {
        format!("{:.2} KB", size as f64 / 1024.0)
    }
}

/// Identifies the appropriate tool for privilege elevation.
fn get_elevation_tool() -> Option<&'static str> {
    if which::which("sudo").is_ok() {
        Some("sudo")
    } else if which::which("doas").is_ok() {
        Some("doas")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mock_paths(root: &Path) -> AppPaths {
        let root = root.to_path_buf();
        AppPaths {
            config_dir: root.join("config"),
            config_file: root.join("config").join(crate::paths::CONFIG_FILE),
            data_dir: root.join("data"),
            state_dir: root.join("state"),
            cache_dir: root.join("cache"),
        }
    }

    #[test]
    fn test_clean_logs() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());
        fs::create_dir_all(paths.log_dir())?;
        let log_file = paths.log_dir().join(crate::paths::LOG_FILE);
        fs::write(&log_file, "some logs")?;

        let cleaner = Cleaner::new(&paths, false);
        cleaner.clean_logs()?;

        assert!(log_file.exists());
        assert_eq!(fs::metadata(log_file)?.len(), 0);
        Ok(())
    }

    #[test]
    fn test_clean_state() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());
        let mut state = State::default();
        let target = dir.path().join("managed.txt");
        fs::write(&target, "content")?;
        let hash = crate::crypto::hash_file(&target)?;
        state.add_file(target.clone(), "test".into(), hash);

        let state_dir = paths.state_file().parent().unwrap().to_path_buf();
        fs::create_dir_all(&state_dir)?;
        state.save(&paths.state_file())?;

        let cleaner = Cleaner::new(&paths, false);
        cleaner.clean_state()?;

        assert!(!target.exists());
        let final_state = State::load(&paths.state_file())?;
        assert!(final_state.managed_files.is_empty());
        Ok(())
    }

    #[test]
    fn test_clean_state_symlinks() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());
        let mut state = State::default();

        let source = dir.path().join("source.txt");
        let target = dir.path().join("link.txt");
        fs::write(&source, "content")?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, &target)?;

        state.add_file(
            target.clone(),
            "link-test".into(),
            format!("symlink:{}", source.display()),
        );

        let state_dir = paths.state_file().parent().unwrap().to_path_buf();
        fs::create_dir_all(&state_dir)?;
        state.save(&paths.state_file())?;

        let cleaner = Cleaner::new(&paths, false);
        cleaner.clean_state()?;

        assert!(!target.exists());
        assert!(source.exists()); // Source should stay
        Ok(())
    }

    #[test]
    fn test_clean_store() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());
        let mut state = State::default();

        let store_dir = paths.store_dir();
        fs::create_dir_all(&store_dir)?;

        let active_art = store_dir.join("activehash-repo");
        let dead_art = store_dir.join("deadhash-repo");
        fs::create_dir_all(&active_art)?;
        fs::create_dir_all(&dead_art)?;

        state.add_file(
            dir.path().join("file"),
            "test".into(),
            "activehash".into(),
        );

        let state_dir = paths.state_file().parent().unwrap().to_path_buf();
        fs::create_dir_all(&state_dir)?;
        state.save(&paths.state_file())?;

        let cleaner = Cleaner::new(&paths, false);
        cleaner.clean_store()?;

        assert!(active_art.exists());
        assert!(!dead_art.exists());

        Ok(())
    }
}
