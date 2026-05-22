use sha2::{Digest, Sha256};

/// Calculates the SHA-256 hash of the given string content.
///
/// Returns a hexadecimal string representation of the hash.
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    let hash =
        result
            .iter()
            .fold(String::with_capacity(64), |mut acc, byte| {
                use std::fmt::Write;
                write!(&mut acc, "{:02x}", byte).ok();
                acc
            });

    let prefix = if hash.len() >= 8 { &hash[..8] } else { &hash };
    tracing::debug!("Content hashed: {}...", prefix);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
