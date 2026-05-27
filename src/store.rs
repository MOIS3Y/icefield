//! Content-addressable store and remote artifact fetching.
//!
//! This module handles downloading files from URLs and extracting archives
//! into a local store located in the application's cache directory.
//! It ensures integrity by verifying SHA-256 hashes.

use crate::paths;
use crate::utils::hash_file;
use anyhow::{Context, Result, anyhow};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tracing::debug;

/// Manages the local store for fetched artifacts.
pub struct Store {
    /// Root path of the store (e.g., ~/.cache/icefield/store).
    root: PathBuf,
}

impl Store {
    /// Creates a new `Store` instance.
    pub fn new(store_dir: &Path) -> Self {
        Self {
            root: store_dir.to_path_buf(),
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
    fn get_content_path(&self, hash: &str, name: &str) -> PathBuf {
        self.root.join(format!("{}-{}", hash, name)).join("out")
    }

    /// Returns the base directory for a given hash and name.
    fn get_base_dir(&self, hash: &str, name: &str) -> PathBuf {
        self.root.join(format!("{}-{}", hash, name))
    }

    /// Ensures the store root directory exists.
    fn ensure_root(&self) -> Result<()> {
        paths::ensure_dir(&self.root)
            .context("Failed to create store root")?;
        Ok(())
    }

    /// Downloads content from a ureq response to a file with a progress bar.
    fn download_to_file(
        &self,
        response: ureq::http::Response<ureq::Body>,
        file: &mut fs::File,
    ) -> Result<()> {
        let content_length = response
            .headers()
            .get("Content-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        let pb = match content_length {
            Some(len) => {
                let pb = ProgressBar::new(len);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("    [{bar:40.blue/white}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")?
                        .progress_chars("=> ")
                );
                pb
            }
            None => {
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .template("    {spinner} {bytes} downloaded...")?,
                );
                pb
            }
        };

        let mut body = response.into_body();
        let mut reader = body.as_reader();
        let mut buffer = [0; 8192];

        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            pb.inc(count as u64);
        }

        pb.finish_and_clear();
        Ok(())
    }

    /// Verifies that the hash of a file matches the expected hash.
    fn verify_hash(
        &self,
        path: &Path,
        expected: &str,
        kind: &str,
        url: &str,
    ) -> Result<()> {
        let actual = hash_file(path)?;
        if actual != expected {
            return Err(anyhow!(
                "hash mismatch for {}: {}\n  specified: {}\n  got:       {}",
                kind,
                url,
                expected,
                actual
            ));
        }
        Ok(())
    }

    /// Fetches a file from a URL and stores it if the hash matches.
    ///
    /// Returns the path to the stored file.
    pub fn fetch_url(
        &self,
        url: &str,
        expected_hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        let name = name.unwrap_or_else(|| Self::extract_name(url));
        let target_path = self.get_content_path(expected_hash, &name);

        if target_path.exists() {
            debug!("Cache hit for URL: {} [{}]", url, &expected_hash[..8]);
            return Ok(target_path);
        }

        debug!("Cache miss for URL: {}. Fetching...", url);
        self.ensure_root()?;

        println!(
            "  {} {} {}",
            console::style("➜").blue(),
            console::style("Fetching:").dim(),
            console::style(url).italic()
        );

        let response = ureq::get(url)
            .call()
            .map_err(|e| anyhow!("Failed to download file: {}", e))?;

        // Create a temp file in the store root to ensure same-device rename
        let temp_file = tempfile::NamedTempFile::new_in(&self.root)?;
        {
            let mut file = fs::File::create(temp_file.path())?;
            self.download_to_file(response, &mut file)?;
        }

        self.verify_hash(temp_file.path(), expected_hash, "URL", url)?;

        // Only create the final directory if the hash matches
        let base_dir = self.get_base_dir(expected_hash, &name);
        paths::ensure_dir(&base_dir)?;
        temp_file.persist(&target_path).map_err(|e| anyhow!(e))?;
        debug!("Stored artifact: {:?}", target_path);
        Ok(target_path)
    }

    /// Fetches a tarball and extracts it into the store.
    ///
    /// Returns the path to the extracted directory.
    pub fn fetch_tarball(
        &self,
        url: &str,
        expected_hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        let name = name.unwrap_or_else(|| Self::extract_name(url));
        let base_dir = self.get_base_dir(expected_hash, &name);
        let out_path = base_dir.join("out");
        let done_marker = base_dir.join(".done");

        if out_path.exists() && done_marker.exists() {
            debug!("Cache hit for tarball: {} [{}]", url, &expected_hash[..8]);
            return Ok(out_path);
        }

        debug!("Cache miss for tarball: {}. Fetching...", url);
        self.ensure_root()?;

        println!(
            "  {} {} {}",
            console::style("➜").blue(),
            console::style("Fetching tarball:").dim(),
            console::style(url).italic()
        );

        let response = ureq::get(url)
            .call()
            .map_err(|e| anyhow!("Failed to download tarball: {}", e))?;

        // Download to a temp file in the store root
        let temp_archive = tempfile::NamedTempFile::new_in(&self.root)?;
        {
            let mut file = fs::File::create(temp_archive.path())?;
            self.download_to_file(response, &mut file)?;
        }

        self.verify_hash(temp_archive.path(), expected_hash, "tarball", url)?;

        // Hash matched, create directory and move the archive
        paths::ensure_dir(&base_dir)?;
        let final_archive = base_dir.join("source.tar.gz");
        temp_archive
            .persist(&final_archive)
            .map_err(|e| anyhow!(e))?;

        if out_path.exists() {
            fs::remove_dir_all(&out_path)?;
        }
        paths::ensure_dir(&out_path)?;

        debug!("Extracting tarball to: {:?}", out_path);
        let tar_gz = fs::File::open(&final_archive)?;
        let tar = flate2::read::GzDecoder::new(tar_gz);
        let mut archive = tar::Archive::new(tar);
        archive
            .unpack(&out_path)
            .context("Failed to unpack tarball")?;

        Self::strip_single_subdir(&out_path)?;

        fs::write(done_marker, "")?;
        debug!("Extraction complete.");
        Ok(out_path)
    }

    /// Fetches a ZIP archive and extracts it into the store.
    ///
    /// Returns the path to the extracted directory.
    pub fn fetch_zip(
        &self,
        url: &str,
        expected_hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        let name = name.unwrap_or_else(|| Self::extract_name(url));
        let base_dir = self.get_base_dir(expected_hash, &name);
        let out_path = base_dir.join("out");
        let done_marker = base_dir.join(".done");

        if out_path.exists() && done_marker.exists() {
            debug!("Cache hit for ZIP: {} [{}]", url, &expected_hash[..8]);
            return Ok(out_path);
        }

        debug!("Cache miss for ZIP: {}. Fetching...", url);
        self.ensure_root()?;

        println!(
            "  {} {} {}",
            console::style("➜").blue(),
            console::style("Fetching ZIP:").dim(),
            console::style(url).italic()
        );

        let response = ureq::get(url)
            .call()
            .map_err(|e| anyhow!("Failed to download ZIP: {}", e))?;

        // Download to a temp file in the store root
        let temp_archive = tempfile::NamedTempFile::new_in(&self.root)?;
        {
            let mut file = fs::File::create(temp_archive.path())?;
            self.download_to_file(response, &mut file)?;
        }

        self.verify_hash(temp_archive.path(), expected_hash, "ZIP", url)?;

        // Hash matched, create directory and move the archive
        paths::ensure_dir(&base_dir)?;
        let final_archive = base_dir.join("source.zip");
        temp_archive
            .persist(&final_archive)
            .map_err(|e| anyhow!(e))?;

        if out_path.exists() {
            fs::remove_dir_all(&out_path)?;
        }
        paths::ensure_dir(&out_path)?;

        debug!("Extracting ZIP to: {:?}", out_path);
        let zip_file = fs::File::open(&final_archive)?;
        let mut archive = zip::ZipArchive::new(zip_file)?;
        archive
            .extract(&out_path)
            .context("Failed to extract ZIP")?;

        Self::strip_single_subdir(&out_path)?;

        fs::write(done_marker, "")?;
        debug!("Extraction complete.");
        Ok(out_path)
    }

    /// Fetches a tarball from GitHub.
    pub fn fetch_from_github(
        &self,
        host: Option<String>,
        owner: &str,
        repo: &str,
        rev: &str,
        hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        let host = host.unwrap_or_else(|| "github.com".to_string());
        let url = format!(
            "https://{}/{}/{}/archive/{}.tar.gz",
            host, owner, repo, rev
        );
        let artifact_name = name.unwrap_or_else(|| repo.to_string());
        self.fetch_tarball(&url, hash, Some(artifact_name))
    }

    /// Fetches a tarball from GitLab.
    pub fn fetch_from_gitlab(
        &self,
        host: Option<String>,
        owner: &str,
        repo: &str,
        rev: &str,
        hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        let host = host.unwrap_or_else(|| "gitlab.com".to_string());
        // GitLab URL format: https://host/owner/repo/-/archive/rev/repo-rev.tar.gz
        let url = format!(
            "https://{}/{}/{}/-/archive/{}/{}-{}.tar.gz",
            host, owner, repo, rev, repo, rev
        );
        let artifact_name = name.unwrap_or_else(|| repo.to_string());
        self.fetch_tarball(&url, hash, Some(artifact_name))
    }

    /// Fetches a tarball from Gitea / Forgejo.
    pub fn fetch_from_gitea(
        &self,
        host: Option<String>,
        owner: &str,
        repo: &str,
        rev: &str,
        hash: &str,
        name: Option<String>,
    ) -> Result<PathBuf> {
        let host = host.unwrap_or_else(|| "gitea.com".to_string());
        let url = format!(
            "https://{}/{}/{}/archive/{}.tar.gz",
            host, owner, repo, rev
        );
        let artifact_name = name.unwrap_or_else(|| repo.to_string());
        self.fetch_tarball(&url, hash, Some(artifact_name))
    }

    /// If a directory contains exactly one subdirectory and nothing else,
    /// moves all contents of that subdirectory into the parent.
    fn strip_single_subdir(path: &Path) -> Result<()> {
        let entries: Vec<_> =
            fs::read_dir(path)?.filter_map(|e| e.ok()).collect();

        if entries.len() == 1 {
            let entry = &entries[0];
            if entry.file_type()?.is_dir() {
                let sub_dir = entry.path();
                debug!("Stripping single subdirectory: {:?}", sub_dir);

                for sub_entry in fs::read_dir(&sub_dir)? {
                    let sub_entry = sub_entry?;
                    let target = path.join(sub_entry.file_name());
                    fs::rename(sub_entry.path(), target)?;
                }
                fs::remove_dir(sub_dir)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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

        let base = store.get_base_dir("1234567890abcdef", "my-repo");
        assert_eq!(
            base,
            PathBuf::from("/cache/store/1234567890abcdef-my-repo")
        );

        let content = store.get_content_path("1234567890abcdef", "my-repo");
        assert_eq!(
            content,
            PathBuf::from("/cache/store/1234567890abcdef-my-repo/out")
        );
    }

    #[test]
    fn test_strip_single_subdir_success() -> Result<()> {
        let dir = tempdir()?;
        let root = dir.path();

        // Setup: root/wrapper_dir/file.txt
        let wrapper = root.join("wrapper_dir");
        fs::create_dir(&wrapper)?;
        fs::write(wrapper.join("file.txt"), "hello")?;
        fs::create_dir(wrapper.join("inner_dir"))?;

        // Run
        Store::strip_single_subdir(root)?;

        // Assert: wrapper_dir is gone, file.txt and inner_dir are at root
        assert!(!wrapper.exists());
        assert!(root.join("file.txt").exists());
        assert!(root.join("inner_dir").is_dir());
        assert_eq!(fs::read_to_string(root.join("file.txt"))?, "hello");

        Ok(())
    }

    #[test]
    fn test_strip_single_subdir_no_action_multiple_items() -> Result<()> {
        let dir = tempdir()?;
        let root = dir.path();

        // Setup: root/dir1 and root/dir2
        fs::create_dir(root.join("dir1"))?;
        fs::create_dir(root.join("dir2"))?;

        Store::strip_single_subdir(root)?;

        // Assert: Both still exist
        assert!(root.join("dir1").exists());
        assert!(root.join("dir2").exists());

        Ok(())
    }

    #[test]
    fn test_strip_single_subdir_no_action_single_file() -> Result<()> {
        let dir = tempdir()?;
        let root = dir.path();

        // Setup: root/just_a_file.txt
        fs::write(root.join("just_a_file.txt"), "hello")?;

        Store::strip_single_subdir(root)?;

        // Assert: File is still there
        assert!(root.join("just_a_file.txt").exists());

        Ok(())
    }
}
