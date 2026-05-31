//! Logic for calculating deterministic fingerprints of local and remote resources.

use crate::crypto;
use crate::fetch::Fetcher;
use anyhow::{Result, anyhow};
use std::path::Path;
use tempfile::tempdir;

/// Responsible for calculating fingerprints of various resources.
pub struct Fingerprint {
    fetcher: Fetcher,
}

impl Fingerprint {
    /// Creates a new `Fingerprint` instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fetcher: Fetcher::new(),
        }
    }

    /// Calculates the fingerprint of the given target (path or URL).
    ///
    /// # Errors
    ///
    /// Returns an error if the target cannot be reached, downloaded, or hashed.
    pub fn calculate(&self, target: &str) -> Result<String> {
        if self.is_remote(target) {
            self.calculate_remote(target)
        } else {
            self.calculate_local(Path::new(target))
        }
    }

    /// Determines if the given target is a remote resource.
    fn is_remote(&self, target: &str) -> bool {
        target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("github:")
            || target.starts_with("gitlab:")
            || target.starts_with("gitea:")
    }

    /// Calculates the fingerprint of a local file or directory.
    fn calculate_local(&self, path: &Path) -> Result<String> {
        if !path.exists() {
            return Err(anyhow!("Path does not exist: {:?}", path));
        }
        if path.is_file() {
            crypto::hash_file(path).map_err(|e| anyhow!(e))
        } else {
            crypto::hash_directory(path)
        }
    }

    /// Calculates the fingerprint of a remote resource by downloading it.
    fn calculate_remote(&self, url: &str) -> Result<String> {
        let tmp = tempdir()?;

        // Handle provider shorthands
        if url.starts_with("github:")
            || url.starts_with("gitlab:")
            || url.starts_with("gitea:")
        {
            let expanded_url = Fetcher::expand_provider_url(url)?;
            self.fetcher.fetch_archive(&expanded_url, tmp.path())?;
            return crypto::hash_directory(tmp.path());
        }

        // Handle direct archive URLs
        if url.ends_with(".tar.gz") || url.ends_with(".zip") {
            self.fetcher.fetch_archive(url, tmp.path())?;
            return crypto::hash_directory(tmp.path());
        }

        // Default to single file fetch
        let temp_file = tempfile::NamedTempFile::new()?;
        let mut file = temp_file.reopen()?;
        self.fetcher.fetch_file(url, &mut file)?;
        crypto::hash_file(temp_file.path()).map_err(|e| anyhow!(e))
    }
}

impl Default for Fingerprint {
    fn default() -> Self {
        Self::new()
    }
}
