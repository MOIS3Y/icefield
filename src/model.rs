//! Core data models for the Icefield configuration system.
//!
//! This module defines the structures that represent the "Desired State" of
//! the system. These structures are deserialized from Lua tables and used
//! throughout the compute and commit phases.

use serde::Deserialize;
use std::path::PathBuf;

/// Common configuration metadata available in any constructor.
///
/// Every derivation, regardless of its type, must provide these fields.
/// They control where the file is placed and what its system attributes are.
#[derive(Deserialize, Debug)]
pub struct CommonMeta {
    /// A descriptive name for this derivation (used in logs and progress bars).
    pub name: String,

    /// Whether this derivation is active.
    ///
    /// If false, the derivation is ignored during the Commit phase, and its
    /// target path is marked for garbage collection. Note that Lua-side
    /// computations (like rendering) passed to 'src' will still execute;
    /// use Lua 'if' statements for maximum performance.
    pub enable: bool,

    /// The final destination path on the filesystem.
    pub dst: PathBuf,

    /// If true, always overwrite the file even if the content hash hasn't changed.
    pub force: Option<bool>,

    /// Whether to use elevated privileges (sudo/doas) for writing this file.
    pub sudo: Option<bool>,
    /// The system user who should own the file (requires `sudo`).
    pub owner: Option<String>,
    /// The system group that should own the file (requires `sudo`).
    pub group: Option<String>,
    /// Unix file mode (permissions) as an octal string, e.g., "0644" or "755".
    pub mode: Option<String>,
}

/// Specific data depending on the chosen constructor.
///
/// This enum represents the various types of configurations Icefield can handle.
/// The `tag = "type"` attribute allows `serde` to dispatch based on the hidden
/// type field injected by the Lua engine.
#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DerivationKind {
    /// Writes raw, formatted text directly to a file.
    Text {
        /// The string content to write.
        src: String,
    },
    /// Physically copies a file from the configuration repository to the target.
    Copy {
        /// Path to the source file in the configuration repository.
        src: PathBuf,
    },
    /// Creates a symbolic link to an existing file or directory.
    Symlink {
        /// The source path the link should point to.
        src: PathBuf,
    },
}

/// Unified structure obtained from the Lua context after execution.
///
/// This is the final result of Phase 1 (Compute). It combines common metadata
/// with a specific derivation kind using `serde(flatten)`.
#[derive(Deserialize, Debug)]
pub struct Derivation {
    /// Metadata common to all derivation types.
    #[serde(flatten)]
    pub meta: CommonMeta,
    /// The specific type and data of this derivation.
    #[serde(flatten)]
    pub kind: DerivationKind,
}
