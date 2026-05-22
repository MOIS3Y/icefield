use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Common configuration metadata available in any constructor.
#[derive(Deserialize, Debug)]
pub struct CommonMeta {
    pub name: String,
    pub target: PathBuf,
    #[allow(dead_code)]
    pub sudo: Option<bool>,
    #[allow(dead_code)]
    pub owner: Option<String>,
    #[allow(dead_code)]
    pub group: Option<String>,
    pub mode: Option<u32>,
    pub executable: Option<bool>,
}

/// Specific data depending on the chosen constructor.
#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DerivationKind {
    Toml {
        source: serde_json::Value,
    },
    Yaml {
        source: serde_json::Value,
    },
    Json {
        source: serde_json::Value,
    },
    Env {
        source: HashMap<String, String>,
    },
    Ini {
        source: HashMap<String, HashMap<String, String>>,
    },
    Symlink {
        source_path: PathBuf,
    },
    Scss {
        template_path: PathBuf,
        variables: serde_json::Value,
    },
    Template {
        template_path: PathBuf,
        variables: serde_json::Value,
    },
}

/// Unified structure obtained from the Lua context after execution.
#[derive(Deserialize, Debug)]
pub struct Derivation {
    #[serde(flatten)]
    pub meta: CommonMeta,
    #[serde(flatten)]
    pub kind: DerivationKind,
}
