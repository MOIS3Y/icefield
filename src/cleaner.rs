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
    pub fn new(paths: &AppPaths, dry_run: bool) -> Self {
        Self {
            paths: paths.clone(),
            dry_run,
        }
    }

    /// Executes the specified cleanup target.
    pub fn execute(&self, target: &CleanTarget) -> Result<()> {
        match target {
            CleanTarget::Logs => self.clean_logs()?,
            CleanTarget::State => self.clean_state()?,
            CleanTarget::Store { force } => {
                self.clean_store(*force)?;
            }
            CleanTarget::All => {
                self.clean_state()?;
                self.clean_store(true)?;
                self.clean_logs()?;
                println!("\n{} System is clean!", style("★").magenta());
            }
        }
        Ok(())
    }

    /// Truncates the central log file to zero bytes.
    fn clean_logs(&self) -> Result<()> {
        println!("{} Clearing logs...", style("≡").cyan());
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

        for (target_path, file_state) in state.managed_files.iter() {
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

    /// Checks if a file or symlink has been modified manually.
    fn is_file_modified(&self, path: &Path, expected_hash: &str) -> bool {
        if expected_hash.starts_with("symlink:") {
            let expected_target =
                expected_hash.strip_prefix("symlink:").unwrap();
            let expected_target_path = Path::new(expected_target);
            let canonical_expected = fs::canonicalize(expected_target_path)
                .unwrap_or_else(|_| expected_target_path.to_path_buf());

            return match fs::symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    match fs::read_link(path) {
                        Ok(actual_target) => {
                            actual_target.to_string_lossy()
                                != canonical_expected.to_string_lossy()
                        }
                        Err(_) => true,
                    }
                }
                _ => true,
            };
        }

        if fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return true; // Expected regular file, found symlink
        }

        if path.is_file() {
            if let Ok(current_hash) = hash_file(path) {
                return current_hash != expected_hash;
            }
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
        if let Some(tool) = get_elevation_tool() {
            if duct::cmd!(tool, "rm", path).run().is_ok() {
                println!(
                    "  {} Removed (elevated) {}",
                    style("✓").green(),
                    path.display()
                );
                info!("Uninstalled file (elevated): {:?}", path);
                return true;
            }
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

    /// Helper to recursively remove empty directories up the tree.
    fn remove_empty_dirs_recursively(&self, dir: &Path) {
        let mut current = dir;
        loop {
            match fs::remove_dir(current) {
                Ok(_) => {
                    info!("Removed empty directory: {:?}", current);
                    if let Some(parent) = current.parent() {
                        current = parent;
                    } else {
                        break;
                    }
                }
                Err(_) => break, // Stop ascending when not empty
            }
        }
    }

    /// Finalizes the state cleanup by wiping the database file.
    fn finalize_state_cleanup(
        &self,
        state_file: &PathBuf,
        removed_count: usize,
        skipped_count: usize,
    ) -> Result<()> {
        if self.dry_run {
            println!("  Would clear state database: {}", state_file.display());
            return Ok(());
        }

        let empty_state = State::default();
        empty_state.save(state_file)?;

        if removed_count == 0 && skipped_count == 0 {
            println!(
                "  {} System state was already clean.",
                style("✓").green()
            );
        }
        Ok(())
    }

    /// Smart GC: Removes artifacts from the store that are not referenced.
    fn clean_store(&self, force: bool) -> Result<()> {
        println!("{} Emptying store cache...", style("⨯").red());
        let store_dir = self.paths.store_dir();

        if !store_dir.exists() {
            println!("  {} Store is already empty.", style("✓").green());
            return Ok(());
        }

        if force {
            return self.wipe_store_completely(&store_dir);
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

    /// Forcefully wipes the entire store directory.
    fn wipe_store_completely(&self, store_dir: &Path) -> Result<()> {
        let size = get_dir_size(store_dir);
        if self.dry_run {
            println!(
                "  Would force-wipe entire store: {} ({})",
                store_dir.display(),
                format_size(size)
            );
        } else {
            fs::remove_dir_all(store_dir).with_context(|| {
                format!("Failed to force wipe store: {:?}", store_dir)
            })?;
            println!(
                "  {} Force wiped store. Freed {}",
                style("✓").green(),
                format_size(size)
            );
        }
        Ok(())
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

        for (target, file_state) in &state.managed_files {
            // Check symlink targets
            if file_state.hash.starts_with("symlink:") {
                let link_target = &file_state.hash[8..];
                if let Ok(rel) = Path::new(link_target).strip_prefix(store_dir)
                {
                    if let Some(folder) = rel.iter().next() {
                        active.insert(folder.to_os_string());
                    }
                }
            }

            // Also check if the managed file itself is inside the store
            if let Ok(rel) = target.strip_prefix(store_dir) {
                if let Some(folder) = rel.iter().next() {
                    active.insert(folder.to_os_string());
                }
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

/// Helper to calculate the recursive size of a directory.
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

/// Helper to format bytes into a human-readable string (KB/MB).
fn format_size(size: u64) -> String {
    let mb = size as f64 / 1_048_576.0;
    if mb >= 1.0 {
        format!("{:.2} MB", mb)
    } else {
        let kb = size as f64 / 1024.0;
        format!("{:.2} KB", kb)
    }
}

/// Detects the available privilege elevation tool (sudo/doas).
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

    fn mock_paths(base: &Path) -> AppPaths {
        AppPaths {
            config_dir: base.join("config"),
            config_file: base.join("config").join("init.lua"),
            data_dir: base.join("data"),
            state_dir: base.join("state"),
            cache_dir: base.join("cache"),
        }
    }

    #[test]
    fn test_clean_logs() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());
        let log_dir = paths.log_dir();
        fs::create_dir_all(&log_dir)?;
        let log_file = log_dir.join(crate::paths::LOG_FILE);

        // Create log file with some data
        fs::write(&log_file, "test log data")?;
        assert_eq!(fs::metadata(&log_file)?.len(), 13);

        // Dry run
        let cleaner_dry = Cleaner::new(&paths, true);
        cleaner_dry.clean_logs()?;
        assert_eq!(fs::metadata(&log_file)?.len(), 13); // Unchanged

        // Actual run
        let cleaner = Cleaner::new(&paths, false);
        cleaner.clean_logs()?;
        assert_eq!(fs::metadata(&log_file)?.len(), 0); // Truncated

        Ok(())
    }

    #[test]
    fn test_clean_state() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());

        let managed_dir = dir.path().join("managed");
        fs::create_dir_all(&managed_dir)?;
        let file1 = managed_dir.join("file1.txt");
        let file2 = managed_dir.join("file2.txt");

        fs::write(&file1, "content1")?;
        let hash1 = hash_file(&file1)?;

        fs::write(&file2, "content2")?;
        let hash2 = hash_file(&file2)?;

        let mut state = State::default();
        state.add_file(file1.clone(), "der1".to_string(), hash1);
        state.add_file(file2.clone(), "der2".to_string(), hash2);

        let state_dir = paths.state_file().parent().unwrap().to_path_buf();
        fs::create_dir_all(&state_dir)?;
        state.save(&paths.state_file())?;

        // Modify file2 so its hash differs
        fs::write(&file2, "modified content")?;

        let cleaner = Cleaner::new(&paths, false);
        cleaner.clean_state()?;

        assert!(!file1.exists()); // Should be deleted
        assert!(file2.exists()); // Should be skipped

        let new_state = State::load(&paths.state_file())?;
        assert!(new_state.managed_files.is_empty()); // State is wiped

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_clean_state_symlinks() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());

        let target_dir = dir.path().join("out");
        let source_dir = dir.path().join("src");
        fs::create_dir_all(&target_dir)?;
        fs::create_dir_all(&source_dir)?;

        let link_ok = target_dir.join("link_ok");
        let link_wrong = target_dir.join("link_wrong");
        let link_to_file = target_dir.join("link_to_file");
        let src_file = source_dir.join("source.txt");
        fs::write(&src_file, "content")?;

        // 1. Create a correct symlink
        std::os::unix::fs::symlink(&src_file, &link_ok)?;

        // 2. Create a symlink pointing to the wrong place
        std::os::unix::fs::symlink(dir.path(), &link_wrong)?;

        // 3. Create a regular file where a symlink is expected
        fs::write(&link_to_file, "i am not a link")?;

        let mut state = State::default();
        state.add_file(
            link_ok.clone(),
            "ok".into(),
            format!("symlink:{}", src_file.display()),
        );
        state.add_file(
            link_wrong.clone(),
            "wrong".into(),
            format!("symlink:{}", src_file.display()),
        );
        state.add_file(
            link_to_file.clone(),
            "to_file".into(),
            format!("symlink:{}", src_file.display()),
        );

        let state_dir = paths.state_file().parent().unwrap().to_path_buf();
        fs::create_dir_all(&state_dir)?;
        state.save(&paths.state_file())?;

        let cleaner = Cleaner::new(&paths, false);
        cleaner.clean_state()?;

        assert!(!link_ok.exists() && fs::symlink_metadata(&link_ok).is_err());
        assert!(fs::read_link(&link_wrong).is_ok()); // Kept (wrong target)
        assert!(link_to_file.is_file()); // Kept (wrong type)

        Ok(())
    }

    #[test]
    fn test_clean_store() -> Result<()> {
        let dir = tempdir()?;
        let paths = mock_paths(dir.path());

        let store_dir = paths.store_dir();
        fs::create_dir_all(&store_dir)?;

        let active_art = store_dir.join("active-art");
        let dead_art = store_dir.join("dead-art");
        fs::create_dir_all(&active_art)?;
        fs::create_dir_all(&dead_art)?;
        fs::write(active_art.join("file.txt"), "active")?;
        fs::write(dead_art.join("file.txt"), "dead")?;

        // Setup state to reference the active artifact
        let mut state = State::default();
        state.add_file(
            dir.path().join("link"),
            "active".into(),
            format!("symlink:{}", active_art.join("file.txt").display()),
        );

        let state_dir = paths.state_file().parent().unwrap().to_path_buf();
        fs::create_dir_all(&state_dir)?;
        state.save(&paths.state_file())?;

        let cleaner = Cleaner::new(&paths, false);
        cleaner.clean_store(false)?;

        assert!(active_art.exists());
        assert!(!dead_art.exists());

        Ok(())
    }
}
