//! Content-addressable store and remote artifact fetching.
//!
//! This module handles downloading files from URLs and extracting archives
//! into a local store located in the application's cache directory.
//! It ensures integrity by verifying SHA-256 hashes and deterministic fingerprints.

use crate::{fetch::Fetcher, paths};
use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Manages the local store for fetched artifacts.
pub struct Store {
    /// Root path of the store (e.g., ~/.cache/icefield/store).
    root: PathBuf,
    fetcher: Fetcher,
}

impl Store {
    /// Creates a new `Store` instance.
    pub fn new(store_dir: &Path) -> Self {
        Self {
            root: store_dir.to_path_buf(),
            fetcher: Fetcher::new(),
        }
    }

    /// Extracts a filename from a URL or returns a default.
    fn extract_name(url: &str) -> String {
        url.split('/')
            .next_back()
            .filter(|s| !s.is_empty())
            .unwrap_or("artifact")
            .to_string()
    }

    /// Returns the path to the content for a given hash and name.
    ///
    /// This path points to the actual content directory (`out`) inside the artifact folder.
    fn get_content_path(&self, hash: &str, name: &str) -> PathBuf {
        self.root.join(format!("{}-{}", hash, name)).join("out")
    }

    /// Returns the base directory for a given hash and name.
    ///
    /// The base directory contains the `out` directory and a `.done` marker.
    fn get_base_dir(&self, hash: &str, name: &str) -> PathBuf {
        self.root.join(format!("{}-{}", hash, name))
    }

    /// Ensures the store root directory exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    fn ensure_root(&self) -> Result<()> {
        paths::ensure_dir(&self.root)
            .context("Failed to create store root")?;
        Ok(())
    }

    /// Fetches a single file from a URL and stores it if the hash matches.
    ///
    /// If the file is already in the store, it returns the cached path.
    ///
    /// # Errors
    ///
    /// Returns an error if the download fails, the hash mismatches, or filesystem operations fail.
    pub fn fetch_url(
        &self,
        url: &str,
        expected_hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        let name = name.unwrap_or_else(|| Self::extract_name(url));
        let target_path = self.get_content_path(expected_hash, &name);

        if target_path.exists() {
            debug!(
                "Cache hit for URL: {} [{}]",
                url,
                &expected_hash[..8.min(expected_hash.len())]
            );
            return Ok(target_path);
        }

        debug!("Cache miss for URL: {}. Fetching...", url);
        self.ensure_root()?;

        let temp_file = tempfile::NamedTempFile::new_in(&self.root)?;
        let mut file = temp_file.reopen()?;
        self.fetcher.fetch_file(url, &mut file)?;

        let actual = crate::crypto::hash_file(temp_file.path())?;
        if actual != expected_hash {
            return Err(anyhow!(
                "hash mismatch for URL: {}\n  specified: {}\n  got:       {}",
                url,
                expected_hash,
                actual
            ));
        }

        let base_dir = self.get_base_dir(expected_hash, &name);
        paths::ensure_dir(&base_dir)?;
        temp_file.persist(&target_path).map_err(|e| anyhow!(e))?;
        debug!("Stored artifact: {:?}", target_path);
        Ok(target_path)
    }

    /// Fetches a remote archive or git repo, extracts it, verifies the deterministic directory fingerprint,
    /// and stores it in the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the fetching action fails or the directory fingerprint mismatches.
    fn fetch_dir<F>(
        &self,
        url: &str,
        expected_hash: &str,
        name: Option<String>,
        kind: &str,
        fetch_action: F,
    ) -> Result<PathBuf>
    where
        F: FnOnce(&Path) -> Result<()>,
    {
        let name = name.unwrap_or_else(|| Self::extract_name(url));
        let base_dir = self.get_base_dir(expected_hash, &name);
        let out_path = base_dir.join("out");
        let done_marker = base_dir.join(".done");

        if out_path.exists() && done_marker.exists() {
            debug!(
                "Cache hit for {}: {} [{}]",
                kind,
                url,
                &expected_hash[..8.min(expected_hash.len())]
            );
            return Ok(out_path);
        }

        debug!("Cache miss for {}: {}. Fetching...", kind, url);
        self.ensure_root()?;

        let temp_dir = tempfile::Builder::new()
            .prefix("ice-store-tmp-")
            .tempdir_in(&self.root)?;

        fetch_action(temp_dir.path())?;

        let actual_hash = crate::crypto::hash_directory(temp_dir.path())?;
        if actual_hash != expected_hash {
            return Err(anyhow!(
                "Fingerprint mismatch for {}: {}\n  specified: {}\n  got:       {}",
                kind,
                url,
                expected_hash,
                actual_hash
            ));
        }

        paths::ensure_dir(&base_dir)?;
        if out_path.exists() {
            fs::remove_dir_all(&out_path)?;
        }

        fs::rename(temp_dir.path(), &out_path)?;
        fs::write(done_marker, "")?;
        debug!("Extraction complete.");
        Ok(out_path)
    }

    /// Fetches a tarball and extracts it into the store.
    ///
    /// # Errors
    ///
    /// Returns an error if the download or extraction fails.
    pub fn fetch_tarball(
        &self,
        url: &str,
        expected_hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        self.fetch_dir(url, expected_hash, name, "tarball", |dest| {
            self.fetcher.fetch_archive(url, dest)
        })
    }

    /// Fetches a ZIP archive and extracts it into the store.
    ///
    /// # Errors
    ///
    /// Returns an error if the download or extraction fails.
    pub fn fetch_zip(
        &self,
        url: &str,
        expected_hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        self.fetch_dir(url, expected_hash, name, "ZIP", |dest| {
            self.fetcher.fetch_archive(url, dest)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_name() {
        assert_eq!(
            Store::extract_name("https://example.com/file.txt"),
            "file.txt"
        );
        assert_eq!(
            Store::extract_name("https://example.com/archive.tar.gz"),
            "archive.tar.gz"
        );
        assert_eq!(
            Store::extract_name("https://example.com/some/path/"),
            "artifact"
        ); // ends with slash
        assert_eq!(Store::extract_name("https://example.com"), "example.com"); // no path
    }

    #[test]
    fn test_get_paths() {
        let store = Store::new(Path::new("/cache/store"));

        let base =
            store.get_base_dir("00000000000000000000000000000000", "my-repo");
        assert_eq!(
            base,
            PathBuf::from(
                "/cache/store/00000000000000000000000000000000-my-repo"
            )
        );

        let content = store
            .get_content_path("00000000000000000000000000000000", "my-repo");
        assert_eq!(
            content,
            PathBuf::from(
                "/cache/store/00000000000000000000000000000000-my-repo/out"
            )
        );
    }
}
