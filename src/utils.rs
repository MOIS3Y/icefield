//! Common utility functions for Icefield.
//!
//! This module provides shared functionality for hashing content and files,
//! which is essential for determining when a configuration file has changed
//! and needs to be updated.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Friendly Base32 alphabet (Nix-style: excludes e, o, u, t).
const BASE32_ALPHABET: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// Encodes 32 bytes (256 bits) into a 52-character Base32 string.
///
/// This uses the Nix-style alphabet and bit-shifting to ensure a compact,
/// human-friendly, and deterministic representation of SHA-256 hashes.
fn to_base32(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(52);
    // 256 bits / 5 bits per character = 51.2 characters -> 52 total.
    for i in (0..52).rev() {
        let b: usize = i * 5;
        let c: usize = b / 8;
        let j: usize = b % 8;

        // Extract 5 bits starting from bit `b`
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
/// This function is used to fingerprint generated text configurations
/// (like TOML, JSON, or rendered templates) to check for changes against
/// the previous state.
///
/// # Returns
///
/// A 52-character Base32 string representation of the hash.
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    let hash = to_base32(&result);

    let prefix = if hash.len() >= 8 { &hash[..8] } else { &hash };
    tracing::debug!("Content hashed (base32): {}...", prefix);
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
/// A 52-character Base32 string representation of the hash.
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
    let hash = to_base32(&result);

    let prefix = if hash.len() >= 8 { &hash[..8] } else { &hash };
    tracing::debug!("File hashed (base32) ({:?}): {}...", path, prefix);
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_hash_empty_string() {
        // SHA-256 for empty string in our Base32
        let expected = "0mdqa9w1p6cmli6976v4wi0sw9r4p5prkj7lzfd1877wk11c9c73";
        assert_eq!(hash_content(""), expected);
        assert_eq!(expected.len(), 52);
    }

    #[test]
    fn test_hash_hello_world() {
        // echo -n "hello world" | sha256sum -> b94d27...
        let expected = "1sfdxziarxw8j3p80lvswgpq9i7smdyxmmsj5sjhhgjdjfwjfkdr";
        assert_eq!(hash_content("hello world"), expected);
        assert_eq!(expected.len(), 52);
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
