//! Remote fetching mechanisms.
//!
//! This module abstracts the downloading of remote configurations and artifacts.
//! It supports both zero-dependency HTTP archive downloads (tar.gz, zip) and
//! system Git shallow cloning.

use anyhow::{Context, Result, anyhow, bail};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

/// Responsible for downloading remote configurations and artifacts.
///
/// It maintains an internal `ureq::Agent` to reuse connections across multiple
/// fetch requests, improving performance through connection pooling.
pub struct Fetcher {
    agent: ureq::Agent,
}

impl Fetcher {
    /// Creates a new `Fetcher` instance with a default connection agent.
    pub fn new() -> Self {
        let agent = ureq::Agent::new_with_defaults();
        Self { agent }
    }

    /// Downloads a single file from a URL to a given destination file.
    ///
    /// # Errors
    ///
    /// Returns an error if the network request fails or the file cannot be written.
    pub fn fetch_file(&self, url: &str, dest: &mut fs::File) -> Result<()> {
        println!(
            "  {} {} {}",
            console::style("➜").blue(),
            console::style("Downloading file:").dim(),
            console::style(url).italic()
        );

        let response = self
            .agent
            .get(url)
            .header(
                "User-Agent",
                format!("Icefield/{}", env!("CARGO_PKG_VERSION")),
            )
            .call()
            .map_err(|e| anyhow!("Failed to download file: {}", e))?;

        self.download_to_file(response, dest)
    }

    /// Downloads an archive from a URL and extracts it into the target directory.
    ///
    /// Supported formats: `.tar.gz`, `.zip`.
    ///
    /// # Errors
    ///
    /// Returns an error if the network request fails or extraction fails.
    pub fn fetch_archive(&self, url: &str, dest: &Path) -> Result<()> {
        println!(
            "  {} {} {}",
            console::style("➜").blue(),
            console::style("Downloading archive:").dim(),
            console::style(url).italic()
        );

        let response = self
            .agent
            .get(url)
            .header(
                "User-Agent",
                format!("Icefield/{}", env!("CARGO_PKG_VERSION")),
            )
            .call()
            .map_err(|e| anyhow!("Failed to download archive: {}", e))?;

        let temp_archive = tempfile::NamedTempFile::new()?;
        let mut file = temp_archive.reopen()?;

        self.download_to_file(response, &mut file)?;

        if !dest.exists() {
            fs::create_dir_all(dest)?;
        }

        if url.ends_with(".zip") {
            let zip_file = fs::File::open(temp_archive.path())?;
            let mut archive = zip::ZipArchive::new(zip_file)?;
            archive.extract(dest).context("Failed to extract ZIP")?;
        } else {
            // Assume tar.gz by default
            let tar_gz = fs::File::open(temp_archive.path())?;
            let tar = flate2::read::GzDecoder::new(tar_gz);
            let mut archive = tar::Archive::new(tar);
            archive.unpack(dest).context("Failed to unpack tarball")?;
        }

        // If there is only one root directory in the archive, strip it
        self.strip_single_subdir(dest)?;

        Ok(())
    }

    /// Helper to stream response body to a file while showing a progress bar.
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

    /// If a directory contains exactly one subdirectory and nothing else,
    /// moves all contents of that subdirectory into the parent.
    /// This handles GitHub archives which wrap contents in `repo-name-hash/`.
    fn strip_single_subdir(&self, path: &Path) -> Result<()> {
        let entries: Vec<_> =
            fs::read_dir(path)?.filter_map(|e| e.ok()).collect();

        if entries.len() == 1 {
            let entry = &entries[0];
            if entry.file_type()?.is_dir() {
                let sub_dir = entry.path();
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

    /// Generates a tarball URL for GitHub.
    pub fn github_url(
        host: Option<&str>,
        owner: &str,
        repo: &str,
        rev: &str,
    ) -> String {
        let host = host.unwrap_or("github.com");
        format!("https://{}/{}/{}/archive/{}.tar.gz", host, owner, repo, rev)
    }

    /// Generates a tarball URL for GitLab.
    pub fn gitlab_url(
        host: Option<&str>,
        owner: &str,
        repo: &str,
        rev: &str,
    ) -> String {
        let host = host.unwrap_or("gitlab.com");
        format!(
            "https://{}/{}/{}/-/archive/{}/{}-{}.tar.gz",
            host, owner, repo, rev, repo, rev
        )
    }

    /// Generates a tarball URL for Gitea / Forgejo.
    pub fn gitea_url(
        host: Option<&str>,
        owner: &str,
        repo: &str,
        rev: &str,
    ) -> String {
        let host = host.unwrap_or("gitea.com");
        format!("https://{}/{}/{}/archive/{}.tar.gz", host, owner, repo, rev)
    }

    /// Expands a provider shorthand (e.g. github:user/repo@rev) into a tarball URL.
    /// Supports custom hosts: provider:host/owner/repo@rev or provider:owner/repo@rev
    ///
    /// # Errors
    ///
    /// Returns an error if the shorthand format is invalid or the provider is unknown.
    pub fn expand_provider_url(shorthand: &str) -> Result<String> {
        let (provider, rest) = shorthand.split_once(':').ok_or_else(|| {
            anyhow!("Invalid shorthand format. Expected 'provider:path'")
        })?;

        // 1. Extract revision if present: path@rev
        let (full_path, rev) = if let Some((p, r)) = rest.split_once('@') {
            (p, r)
        } else {
            (rest, "main")
        };

        // 2. Parse path segments
        let segments: Vec<&str> = full_path.split('/').collect();
        let (custom_host, owner, repo) = match segments.len() {
            2 => (None, segments[0], segments[1]),
            3 => (Some(segments[0]), segments[1], segments[2]),
            _ => {
                bail!(
                    "Invalid repository path '{}'. Expected 'owner/repo' or 'host/owner/repo'",
                    full_path
                );
            }
        };

        match provider {
            "github" => Ok(Self::github_url(custom_host, owner, repo, rev)),
            "gitlab" => Ok(Self::gitlab_url(custom_host, owner, repo, rev)),
            "gitea" => Ok(Self::gitea_url(custom_host, owner, repo, rev)),
            _ => bail!("Unknown provider: {}", provider),
        }
    }
}

impl Default for Fetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_url_generators() {
        assert_eq!(
            Fetcher::github_url(None, "rust-lang", "rust", "master"),
            "https://github.com/rust-lang/rust/archive/master.tar.gz"
        );
        assert_eq!(
            Fetcher::gitlab_url(
                Some("gitlab.custom.org"),
                "inkscape",
                "inkscape",
                "v1.2.0"
            ),
            "https://gitlab.custom.org/inkscape/inkscape/-/archive/v1.2.0/inkscape-v1.2.0.tar.gz"
        );
        assert_eq!(
            Fetcher::gitea_url(None, "gitea", "tea", "main"),
            "https://gitea.com/gitea/tea/archive/main.tar.gz"
        );
    }

    #[test]
    fn test_strip_single_subdir_success() -> Result<()> {
        let dir = tempdir()?;
        let root = dir.path();
        let fetcher = Fetcher::new();

        // Setup: root/wrapper_dir/file.txt
        let wrapper = root.join("wrapper_dir");
        fs::create_dir(&wrapper)?;
        fs::write(wrapper.join("file.txt"), "hello")?;
        fs::create_dir(wrapper.join("inner_dir"))?;

        // Run
        fetcher.strip_single_subdir(root)?;

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
        let fetcher = Fetcher::new();

        // Setup: root/dir1 and root/dir2
        fs::create_dir(root.join("dir1"))?;
        fs::create_dir(root.join("dir2"))?;

        fetcher.strip_single_subdir(root)?;

        // Assert: Both still exist
        assert!(root.join("dir1").exists());
        assert!(root.join("dir2").exists());

        Ok(())
    }

    #[test]
    fn test_strip_single_subdir_no_action_single_file() -> Result<()> {
        let dir = tempdir()?;
        let root = dir.path();
        let fetcher = Fetcher::new();

        // Setup: root/just_a_file.txt
        fs::write(root.join("just_a_file.txt"), "hello")?;

        fetcher.strip_single_subdir(root)?;

        // Assert: File is still there
        assert!(root.join("just_a_file.txt").exists());

        Ok(())
    }
}
