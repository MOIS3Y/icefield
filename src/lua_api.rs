//! Icefield Lua Standard Library.
//!
//! This module provides a set of helper functions and system information
//! exposed to the user's Lua configuration via the global `icefield` table.
//! It includes tools for path expansion, command execution, and system
//! inspection, organized into logical sub-tables:
//!
//! - `sys`: System information (OS, hostname, username) and command execution.
//! - `fs`: Filesystem utilities (path expansion, existence checks, directory
//!   locations).
//! - `format`: Data serialization and parsing (JSON, TOML, YAML).
//! - `fetch`: Remote artifact downloaders (URL, GitHub, GitLab, Gitea).
//! - `drv`: Derivation constructors (TOML, JSON, Copy, Symlink, etc.).
//! - `lib`: High-level utility library containing helper functions for string
//!   manipulation, table processing, hashing, and logic helpers.
use crate::lua_registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use crate::paths;
use crate::store::Store;
use mlua::{Lua, LuaSerdeExt, Result, Table};
use std::path::Path;

/// Registers the `icefield` global table and its structured sub-tables.
///
/// This is the main entry point for preparing the Lua environment with
/// Icefield's built-in API.
///
/// # Errors
///
/// Returns a Lua error if table creation or registration fails.
pub fn register(
    lua: &Lua,
    paths: &paths::AppPaths,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let icefield = lua.create_table()?;

    // --- Sub-table: icefield.sys ---
    let sys = lua.create_table()?;
    registry.register_var(
        &sys,
        LuaApiItem {
            table: "sys",
            name: "os",
            description: "Returns the name of the operating system ('linux', 'macos', or 'unix').",
            kind: LuaItemKind::Variable { type_name: "string" },
        },
        get_os(),
    )?;

    registry.register_var(
        &sys,
        LuaApiItem {
            table: "sys",
            name: "username",
            description: "Returns the name of the currently logged-in user.",
            kind: LuaItemKind::Variable {
                type_name: "string",
            },
        },
        get_username(),
    )?;

    registry.register_var(
        &sys,
        LuaApiItem {
            table: "sys",
            name: "hostname",
            description: "Returns the hostname of the current machine.",
            kind: LuaItemKind::Variable {
                type_name: "string",
            },
        },
        get_hostname(),
    )?;

    registry.register_func(
        &sys,
        lua,
        LuaApiItem {
            table: "sys",
            name: "has_command",
            description: "Checks if a command is available in the system PATH.",
            kind: LuaItemKind::Function {
                params: &[("cmd", "string")],
                returns: "boolean",
            },
        },
        |_, cmd: String| Ok(has_command(&cmd)),
    )?;

    let run_cmd_dir = paths.config_dir.clone();
    registry.register_func(
        &sys,
        lua,
        LuaApiItem {
            table: "sys",
            name: "run_command",
            description: "Executes a shell command and returns its standard output.",
            kind: LuaItemKind::Function {
                params: &[("cmd", "string"), ("args", "table")],
                returns: "string",
            },
        },
        move |_, (cmd, args): (String, Vec<String>)| run_command(&cmd, args, &run_cmd_dir),
    )?;
    icefield.set("sys", sys)?;

    // --- Sub-table: icefield.fs ---
    let fs = lua.create_table()?;
    let cfg_dir = paths.config_dir.clone();
    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "config_dir",
            description: "Returns the absolute path to the Icefield configuration directory.",
            kind: LuaItemKind::Function { params: &[], returns: "string" },
        },
        move |_, ()| Ok(cfg_dir.to_string_lossy().into_owned()),
    )?;

    let cch_dir = paths.cache_dir.clone();
    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "cache_dir",
            description: "Returns the absolute path to the Icefield cache directory.",
            kind: LuaItemKind::Function { params: &[], returns: "string" },
        },
        move |_, ()| Ok(cch_dir.to_string_lossy().into_owned()),
    )?;

    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "home_dir",
            description: "Returns the absolute path to the current user's home directory.",
            kind: LuaItemKind::Function { params: &[], returns: "string" },
        },
        |_, ()| Ok(get_home_dir()),
    )?;

    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "exists",
            description: "Checks if a file or directory exists at the given path.",
            kind: LuaItemKind::Function { params: &[("path", "string")], returns: "boolean" },
        },
        |_, path: String| Ok(path_exists(&path)),
    )?;

    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "expand",
            description: "Expands tildes (`~`) and environment variables in the path.",
            kind: LuaItemKind::Function { params: &[("path", "string")], returns: "string" },
        },
        |_, path: String| Ok(path_expand(&path)),
    )?;
    icefield.set("fs", fs)?;

    // --- Sub-table: icefield.format ---
    register_format_helpers(&icefield, lua, registry)?;

    // --- Sub-table: icefield.fetch ---
    register_fetchers(&icefield, lua, paths, registry)?;

    // --- Sub-table: icefield.drv ---
    register_drv_constructors(&icefield, lua, registry)?;

    // --- Sub-table: icefield.lib ---
    let lib = lua.create_table()?;
    registry.register_func(
        &lib,
        lua,
        LuaApiItem {
            table: "lib",
            name: "fake_hash",
            description: "Returns a dummy 52-character Nix-style Base32 hash, useful for bootstrapping new derivations.",
            kind: LuaItemKind::Function { params: &[], returns: "string" },
        },
        |_, ()| Ok("0000000000000000000000000000000000000000000000000000"),
    )?;

    // Manually register metadata for string.trim (which is injected via bootstrap_lua_env)
    registry.items.push(LuaApiItem {
        table: "lib.string",
        name: "trim",
        description: "Removes leading and trailing whitespace from a string.",
        kind: LuaItemKind::Function {
            params: &[("s", "string")],
            returns: "string",
        },
    });

    icefield.set("lib", lib)?;

    // --- Finalize: icefield table ---
    lua.globals().set("icefield", icefield)?;

    // --- Lua Bootstrap (populates icefield.lib with Lua helpers) ---
    bootstrap_lua_env(lua)?;

    Ok(())
}

/// Helper to wrap fetch errors with a newline for Lua traceback.
fn wrap_fetch_err(e: anyhow::Error, kind: &str) -> mlua::Error {
    mlua::Error::RuntimeError(format!("\nFetch failed ({}): {}", kind, e))
}

/// Registers derivation constructors in the `icefield.drv` table.
///
/// These constructors add a `"type"` tag to the configuration table,
/// allowing Rust to deserialize it into the correct `DerivationKind`.
fn register_drv_constructors(
    icefield: &Table,
    lua: &Lua,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let drv = lua.create_table()?;

    let kinds = [
        ("json", "json"),
        ("yaml", "yaml"),
        ("toml", "toml"),
        ("ini", "ini"),
        ("env", "env"),
        ("text", "text"),
        ("template", "template"),
        ("scss", "scss"),
        ("copy", "copy"),
        ("symlink", "symlink"),
    ];

    for (name, kind_tag) in kinds {
        let desc = Box::leak(
            format!("Constructs a new '{}' derivation.", name)
                .into_boxed_str(),
        );
        registry.register_func(
            &drv,
            lua,
            LuaApiItem {
                table: "drv",
                name,
                description: desc,
                kind: LuaItemKind::Function {
                    params: &[("args", "table")],
                    returns: "table",
                },
            },
            move |_, args: Table| {
                args.set("type", kind_tag)?;
                Ok(args)
            },
        )?;
    }

    icefield.set("drv", drv)?;
    Ok(())
}

/// Registers fetcher functions in the `icefield.fetch` table.
///
/// Fetchers download remote resources and place them in the content-addressable
/// store, verifying their integrity via SHA-256 hashes.
fn register_fetchers(
    icefield: &Table,
    lua: &Lua,
    paths: &paths::AppPaths,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let fetch = lua.create_table()?;
    let sd = paths.store_dir();

    // fetch.url({ url, hash, name? })
    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "url",
            description: "Downloads a file from a URL, verifies its hash, and stores it in the local cache. Returns the local absolute path.",
            kind: LuaItemKind::Function { params: &[("args", "table")], returns: "string" },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_url(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "URL"))?;
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    // fetch.tarball({ url, hash, name? })
    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "tarball",
            description: "Downloads and extracts a tarball (.tar.gz), verifies its hash, and returns the local absolute path to the extracted directory.",
            kind: LuaItemKind::Function { params: &[("args", "table")], returns: "string" },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_tarball(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "tarball"))?;
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    // fetch.zip({ url, hash, name? })
    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "zip",
            description: "Downloads and extracts a ZIP archive, verifies its hash, and returns the local absolute path to the extracted directory.",
            kind: LuaItemKind::Function { params: &[("args", "table")], returns: "string" },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_zip(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "ZIP"))?;
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    // fetch.from_github({ owner, repo, rev, hash, host?, name? })
    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "from_github",
            description: "Fetches a repository archive from GitHub at a specific revision.",
            kind: LuaItemKind::Function { params: &[("args", "table")], returns: "string" },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let host: Option<String> = args.get("host")?;
            let owner: String = args.get("owner")?;
            let repo: String = args.get("repo")?;
            let rev: String = args.get("rev")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_from_github(host, &owner, &repo, &rev, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "GitHub"))?;
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    // fetch.from_gitlab({ owner, repo, rev, hash, host?, name? })
    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "from_gitlab",
            description: "Fetches a repository archive from GitLab at a specific revision.",
            kind: LuaItemKind::Function { params: &[("args", "table")], returns: "string" },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let host: Option<String> = args.get("host")?;
            let owner: String = args.get("owner")?;
            let repo: String = args.get("repo")?;
            let rev: String = args.get("rev")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_from_gitlab(host, &owner, &repo, &rev, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "GitLab"))?;
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    // fetch.from_gitea({ host, owner, repo, rev, hash, name? })
    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "from_gitea",
            description: "Fetches a repository archive from Gitea/Forgejo at a specific revision.",
            kind: LuaItemKind::Function { params: &[("args", "table")], returns: "string" },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let host: Option<String> = args.get("host")?;
            let owner: String = args.get("owner")?;
            let repo: String = args.get("repo")?;
            let rev: String = args.get("rev")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_from_gitea(host, &owner, &repo, &rev, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "Gitea"))?;
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    icefield.set("fetch", fetch)?;
    Ok(())
}

/// Injects high-level Lua wrappers and string extensions into `icefield.lib`.
/// Also extends the global `string` table with the `trim` method for convenience.
fn bootstrap_lua_env(lua: &Lua) -> Result<()> {
    lua.load(
        r#"
        -- Add to global string table for s:trim() support
        function string.trim(s)
            return s:match("^%s*(.-)%s*$")
        end

        -- Also expose via icefield.lib
        local lib = icefield.lib
        lib.string = lib.string or {}
        lib.string.trim = string.trim
    "#,
    )
    .exec()
}

/// Registers data serialization and parsing helpers in the `icefield.format` table.
fn register_format_helpers(
    icefield: &Table,
    lua: &Lua,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let format = lua.create_table()?;

    // JSON
    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "from_json",
            description: "Parses a JSON string into a Lua table.",
            kind: LuaItemKind::Function {
                params: &[("s", "string")],
                returns: "table",
            },
        },
        |lua: &Lua, s: String| {
            let v = parse_json(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "to_json",
            description: "Generates a pretty-printed JSON string from a Lua table.",
            kind: LuaItemKind::Function { params: &[("t", "table")], returns: "string" },
        },
        |lua: &Lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            Ok(serialize_json(&v))
        },
    )?;

    // TOML
    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "from_toml",
            description: "Parses a TOML string into a Lua table.",
            kind: LuaItemKind::Function {
                params: &[("s", "string")],
                returns: "table",
            },
        },
        |lua: &Lua, s: String| {
            let v = parse_toml(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "to_toml",
            description: "Generates a TOML string from a Lua table.",
            kind: LuaItemKind::Function {
                params: &[("t", "table")],
                returns: "string",
            },
        },
        |lua: &Lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            serialize_toml(&v)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        },
    )?;

    // YAML
    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "from_yaml",
            description: "Parses a YAML string into a Lua table.",
            kind: LuaItemKind::Function {
                params: &[("s", "string")],
                returns: "table",
            },
        },
        |lua: &Lua, s: String| {
            let v = parse_yaml(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "to_yaml",
            description: "Generates a YAML string from a Lua table.",
            kind: LuaItemKind::Function {
                params: &[("t", "table")],
                returns: "string",
            },
        },
        |lua: &Lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            serialize_yaml(&v)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        },
    )?;

    icefield.set("format", format)?;
    Ok(())
}

// --- Standalone Testable Functions ---

/// Returns the current operating system name ("linux", "macos", or "unix").
pub fn get_os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unix"
    }
}

/// Returns the current username, or "unknown" if it cannot be determined.
pub fn get_username() -> String {
    whoami::username().unwrap_or_else(|_| "unknown".into())
}

/// Returns the system hostname, or "unknown" if it cannot be determined.
pub fn get_hostname() -> String {
    whoami::hostname().unwrap_or_else(|_| "unknown".into())
}

/// Returns the current user's home directory path as a string.
pub fn get_home_dir() -> String {
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".into())
}

/// Checks if a command exists in the system's PATH.
pub fn has_command(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

/// Checks if a file or directory exists at the specified path.
pub fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Expands tildes (`~`) and environment variables in the given path string.
pub fn path_expand(path: &str) -> String {
    shellexpand::full(path)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// Parses a JSON string into a `serde_json::Value`.
///
/// # Errors
///
/// Returns an error if the string is not valid JSON.
pub fn parse_json(s: &str) -> anyhow::Result<serde_json::Value> {
    serde_json::from_str(s)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a pretty-printed JSON string.
pub fn serialize_json(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Parses a TOML string into a `serde_json::Value`.
///
/// # Errors
///
/// Returns an error if the string is not valid TOML.
pub fn parse_toml(s: &str) -> anyhow::Result<serde_json::Value> {
    toml::from_str(s).map_err(|e| anyhow::anyhow!("TOML parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a TOML string.
///
/// # Errors
///
/// Returns an error if the value cannot be represented as TOML.
pub fn serialize_toml(v: &serde_json::Value) -> anyhow::Result<String> {
    toml::to_string(v)
        .map_err(|e| anyhow::anyhow!("TOML serialize error: {}", e))
}

/// Parses a YAML string into a `serde_json::Value`.
///
/// # Errors
///
/// Returns an error if the string is not valid YAML.
pub fn parse_yaml(s: &str) -> anyhow::Result<serde_json::Value> {
    serde_yaml::from_str(s)
        .map_err(|e| anyhow::anyhow!("YAML parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a YAML string.
///
/// # Errors
///
/// Returns an error if the value cannot be represented as YAML.
pub fn serialize_yaml(v: &serde_json::Value) -> anyhow::Result<String> {
    serde_yaml::to_string(v)
        .map_err(|e| anyhow::anyhow!("YAML serialize error: {}", e))
}

/// Executes an external command, captures its stdout, and returns it.
///
/// Prompts and error messages from the command are redirected to the
/// terminal's stderr.
///
/// # Errors
///
/// Returns a Lua error if the command fails to execute or returns a non-zero
/// exit code.
pub fn run_command(
    cmd: &str,
    args: Vec<String>,
    dir: &Path,
) -> Result<String> {
    use console::style;

    println!(
        "  {} {} {}",
        style("➜").blue(),
        style("Running:").dim(),
        style(format!("{} {}", cmd, args.join(" "))).italic()
    );

    let result = duct::cmd(cmd, args)
        .dir(dir)
        .stdout_capture()
        .unchecked()
        .run();

    match result {
        Ok(output) => {
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            } else {
                Err(mlua::Error::RuntimeError(format!(
                    "Command failed with exit code {}: {}",
                    output.status.code().unwrap_or(-1),
                    cmd
                )))
            }
        }
        Err(e) => Err(mlua::Error::RuntimeError(format!(
            "Failed to execute command: {}",
            e
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_name() {
        let os = get_os();
        assert!(os == "linux" || os == "macos" || os == "unix");
    }

    #[test]
    fn test_path_expand() {
        // Safe to unwrap in tests
        unsafe {
            std::env::set_var("TEST_VAR", "ice");
        }
        assert_eq!(path_expand("$TEST_VAR/field"), "ice/field");

        let expanded = path_expand("~/test");
        assert!(expanded.starts_with('/') || expanded.contains(":\\"));
    }

    #[test]
    fn test_has_command() {
        assert!(has_command("ls"));
        assert!(!has_command("non-existent-command-xyz"));
    }

    #[test]
    fn test_json_helpers() {
        let json = r#"{"foo": "bar"}"#;
        let v = parse_json(json).unwrap();
        assert_eq!(v["foo"], "bar");
        assert!(serialize_json(&v).contains("\"foo\": \"bar\""));
    }

    #[test]
    fn test_toml_helpers() {
        let toml = "foo = \"bar\"";
        let v = parse_toml(toml).unwrap();
        assert_eq!(v["foo"], "bar");
        assert_eq!(serialize_toml(&v).unwrap().trim(), "foo = \"bar\"");
    }

    #[test]
    fn test_lua_hierarchy() -> Result<()> {
        let lua = Lua::new();
        let dir = tempfile::tempdir()
            .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
        let config_dir = dir.path().join("cfg");
        let paths = paths::AppPaths::resolve(Some(config_dir));
        let mut registry = crate::lua_registry::ApiRegistry::new();
        register(&lua, &paths, &mut registry)?;

        // Check sys
        let os: String = lua.load("icefield.sys.os").eval()?;
        assert!(os == "linux" || os == "macos" || os == "unix");

        // Check fs
        let home: String = lua.load("icefield.fs.home_dir()").eval()?;
        assert!(!home.is_empty());

        // Check format
        let json: String =
            lua.load("icefield.format.to_json({a=1})").eval()?;
        assert!(json.contains("\"a\": 1"));

        // Check lib.string.trim
        let trimmed: String =
            lua.load("icefield.lib.string.trim('  hello  ')").eval()?;
        assert_eq!(trimmed, "hello");

        // Check lib.fake_hash()
        let hash: String = lua.load("icefield.lib.fake_hash()").eval()?;
        assert_eq!(
            hash,
            "0000000000000000000000000000000000000000000000000000"
        );

        Ok(())
    }
}
