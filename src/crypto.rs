//! Cryptographic Utilities and Hashing.
//!
//! This module provides shared functionality for hashing content, files, and entire
//! directories. It uses a Nix-style Base32 encoding for hashes.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use walkdir::WalkDir;

/// Friendly Base32 alphabet (Nix-style: excludes e, o, u, t).
const BASE32_ALPHABET: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// Encodes 20 bytes (160 bits) into a 32-character Base32 string.
///
/// This uses the Nix-style Base32 encoding which is specifically designed
/// to be human-friendly and avoid ambiguous characters.
fn to_base32(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(32);
    for i in (0..32).rev() {
        let b: usize = i * 5;
        let c: usize = b / 8;
        let j: usize = b % 8;

        let mut v: u16 = (bytes[c] as u16) >> j;
        if c + 1 < bytes.len() {
            v |= (bytes[c + 1] as u16) << (8 - j);
        }
        let val = (v & 0x1f) as usize;
        result.push(BASE32_ALPHABET[val] as char);
    }
    result
}

/// Calculates the SHA-256 hash of the given string content.
///
/// Returns a 32-character Base32 string (truncated to 160 bits, Nix-style).
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();

    // Truncate to 160 bits (20 bytes) for 32-character Base32 representation
    let hash = to_base32(&result[..20]);

    let prefix = if hash.len() >= 8 { &hash[..8] } else { &hash };
    tracing::debug!("Content hashed (base32): {}...", prefix);
    hash
}

/// Calculates the SHA-256 hash of a file on disk.
///
/// Returns a 32-character Base32 string (truncated to 160 bits, Nix-style).
///
/// # Errors
///
/// Returns `std::io::Error` if the file cannot be opened or read.
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

    // Truncate to 160 bits (20 bytes) for 32-character Base32 representation
    let hash = to_base32(&result[..20]);

    let prefix = if hash.len() >= 8 { &hash[..8] } else { &hash };
    tracing::debug!("File hashed (base32) ({:?}): {}...", path, prefix);
    Ok(hash)
}

/// Calculates a deterministic fingerprint for a directory.
///
/// It recurses through the directory and explicitly omitting `.git/` directories.
///
/// # Errors
///
/// Returns an error if any directory entry cannot be read or if file hashing fails.
pub fn hash_directory(dir: &Path) -> Result<String> {
    let mut paths = Vec::new();

    for entry in WalkDir::new(dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
    {
        let entry = entry
            .context("Failed to read directory entry during fingerprinting")?;

        if entry.file_type().is_file() {
            let rel_path =
                entry.path().strip_prefix(dir).unwrap_or(entry.path());
            paths.push(rel_path.to_string_lossy().replace('\\', "/"));
        }
    }

    // Sort paths alphabetically to ensure deterministic hashing order
    paths.sort();

    let mut combined_state = String::new();

    for path_str in paths {
        let full_path = dir.join(&path_str);
        let file_hash = hash_file(&full_path).with_context(|| {
            format!("Failed to hash file: {}", full_path.display())
        })?;

        combined_state.push_str(&path_str);
        combined_state.push('\n');
        combined_state.push_str(&file_hash);
        combined_state.push('\n');
    }

    Ok(hash_content(&combined_state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_hash_empty_string() {
        let expected = "wi0sw9r4p5prkj7lzfd1877wk11c9c73";
        assert_eq!(hash_content(""), expected);
    }

    #[test]
    fn test_hash_hello_world() {
        let expected = "wgpq9i7smdyxmmsj5sjhhgjdjfwjfkdr";
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

    #[test]
    fn test_deterministic_directory_hash() -> Result<()> {
        let dir = tempdir()?;

        // Create files in "random" order
        fs::write(dir.path().join("b.txt"), "content b")?;
        fs::write(dir.path().join("a.txt"), "content a")?;

        // Setup a .git folder with a file
        fs::create_dir(dir.path().join(".git"))?;
        fs::write(dir.path().join(".git/config"), "secret")?;

        let hash1 = hash_directory(dir.path())?;

        // Recreate directory, add files in different order
        let dir2 = tempdir()?;
        fs::write(dir2.path().join("a.txt"), "content a")?;
        fs::write(dir2.path().join("b.txt"), "content b")?;

        let hash2 = hash_directory(dir2.path())?;

        // Hashes should match (deterministic, ignoring .git)
        assert_eq!(hash1, hash2);
        Ok(())
    }
}
