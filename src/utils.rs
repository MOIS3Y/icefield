//! Common utility functions for Icefield.
//!
//! This module provides shared functionality for hashing content and files,
//! which is essential for determining when a configuration file has changed
//! and needs to be updated.

use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Calculates the SHA-256 hash of the given string content.
///
/// This function is used to fingerprint generated text configurations
/// (like TOML, JSON, or rendered templates) to check for changes against
/// the previous state.
///
/// # Returns
///
/// A 64-character hexadecimal string representation of the hash.
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    let hash =
        result
            .iter()
            .fold(String::with_capacity(64), |mut acc, byte| {
                write!(&mut acc, "{:02x}", byte).ok();
                acc
            });

    let prefix = if hash.len() >= 8 { &hash[..8] } else { &hash };
    tracing::debug!("Content hashed: {}...", prefix);
    hash
}

/// Calculates the SHA-256 hash of a file on disk.
///
/// This function reads the file in chunks to efficiently handle large files
/// without consuming excessive memory. It is primarily used for `Copy`
/// derivations.
///
/// # Errors
///
/// Returns an error if the file cannot be opened or read.
///
/// # Returns
///
/// A 64-character hexadecimal string representation of the hash.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    let result = hasher.finalize();
    let hash =
        result
            .iter()
            .fold(String::with_capacity(64), |mut acc, byte| {
                write!(&mut acc, "{:02x}", byte).ok();
                acc
            });

    let prefix = if hash.len() >= 8 { &hash[..8] } else { &hash };
    tracing::debug!("File hashed ({:?}): {}...", path, prefix);
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_hash_empty_string() {
        let expected =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(hash_content(""), expected);
    }

    #[test]
    fn test_hash_hello_world() {
        // echo -n "hello world" | sha256sum
        let expected =
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert_eq!(hash_content("hello world"), expected);
    }

    #[test]
    fn test_hash_file() -> std::io::Result<()> {
        let mut tmp = tempfile::NamedTempFile::new()?;
        write!(tmp, "hello file")?;

        let expected = hash_content("hello file");
        let actual = hash_file(tmp.path())?;
        assert_eq!(actual, expected);
        Ok(())
    }
}
