//! Icefield Lua Standard Library.
//!
//! This module provides a set of helper functions and system information
//! exposed to the user's Lua configuration via the global `icefield` table.
//! It includes tools for path expansion, command execution, and system inspection.

use crate::store::Store;
use mlua::{Lua, LuaSerdeExt, Result, Table};
use std::path::Path;

/// Registers the `icefield` global table and all its helper functions.
pub fn register(lua: &Lua, config_dir: &Path, cache_dir: &Path) -> Result<()> {
    let icefield = lua.create_table()?;

    // 1. System Info
    icefield.set("os", get_os())?;
    icefield.set("username", get_username())?;
    icefield.set("hostname", get_hostname())?;

    // 2. Directory helpers
    let cfg_dir = config_dir.to_path_buf();
    icefield.set(
        "config_dir",
        lua.create_function(move |_, ()| {
            Ok(cfg_dir.to_string_lossy().into_owned())
        })?,
    )?;

    icefield
        .set("home_dir", lua.create_function(|_, ()| Ok(get_home_dir()))?)?;

    // 3. Utility functions
    icefield.set(
        "has_command",
        lua.create_function(|_, cmd: String| Ok(has_command(&cmd)))?,
    )?;
    icefield.set(
        "exists",
        lua.create_function(|_, path: String| Ok(path_exists(&path)))?,
    )?;
    icefield.set(
        "expand",
        lua.create_function(|_, path: String| Ok(path_expand(&path)))?,
    )?;
    icefield.set(
        "fake_hash",
        "0000000000000000000000000000000000000000000000000000",
    )?;

    // 4. Data handling (JSON, TOML, YAML)
    register_data_helpers(&icefield, lua)?;

    // 5. External Commands
    let run_cmd_dir = config_dir.to_path_buf();
    icefield.set(
        "run_command",
        lua.create_function(move |_, (cmd, args): (String, Vec<String>)| {
            run_command(&cmd, args, &run_cmd_dir)
        })?,
    )?;

    // 6. Fetchers
    register_fetchers(&icefield, lua, cache_dir)?;

    lua.globals().set("icefield", icefield)?;

    // 7. Lua Bootstrap
    bootstrap_lua_env(lua)?;

    Ok(())
}

/// Helper to wrap fetch errors with a newline for Lua traceback.
fn wrap_fetch_err(e: anyhow::Error, kind: &str) -> mlua::Error {
    mlua::Error::RuntimeError(format!("\nFetch failed ({}): {}", kind, e))
}

/// Registers fetcher functions in the icefield table.
fn register_fetchers(
    icefield: &Table,
    lua: &Lua,
    cache_dir: &Path,
) -> Result<()> {
    let cache = cache_dir.to_path_buf();

    // fetch_url({ url, hash, name? })
    let c = cache.clone();
    icefield.set(
        "fetch_url",
        lua.create_function(move |_, args: Table| {
            let store = Store::new(&c);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_url(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "URL"))?;
            Ok(path.to_string_lossy().into_owned())
        })?,
    )?;

    // fetch_tarball({ url, hash, name? })
    let c = cache.clone();
    icefield.set(
        "fetch_tarball",
        lua.create_function(move |_, args: Table| {
            let store = Store::new(&c);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_tarball(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "tarball"))?;
            Ok(path.to_string_lossy().into_owned())
        })?,
    )?;

    // fetch_zip({ url, hash, name? })
    let c = cache.clone();
    icefield.set(
        "fetch_zip",
        lua.create_function(move |_, args: Table| {
            let store = Store::new(&c);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            let path = store
                .fetch_zip(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "ZIP"))?;
            Ok(path.to_string_lossy().into_owned())
        })?,
    )?;

    // fetch_from_github({ owner, repo, rev, hash, host?, name? })
    let c = cache.clone();
    icefield.set(
        "fetch_from_github",
        lua.create_function(move |_, args: Table| {
            let store = Store::new(&c);
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
        })?,
    )?;

    // fetch_from_gitlab({ owner, repo, rev, hash, host?, name? })
    let c = cache.clone();
    icefield.set(
        "fetch_from_gitlab",
        lua.create_function(move |_, args: Table| {
            let store = Store::new(&c);
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
        })?,
    )?;

    // fetch_from_gitea({ host, owner, repo, rev, hash, name? })
    let c = cache.clone();
    icefield.set(
        "fetch_from_gitea",
        lua.create_function(move |_, args: Table| {
            let store = Store::new(&c);
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
        })?,
    )?;

    Ok(())
}

/// Injects high-level Lua wrappers and string extensions.
fn bootstrap_lua_env(lua: &Lua) -> Result<()> {
    lua.load(
        r#"
        -- String helpers
        function string.trim(s)
            return s:match("^%s*(.-)%s*$")
        end
    "#,
    )
    .exec()
}

/// Registers data serialization and parsing helpers.
fn register_data_helpers(icefield: &Table, lua: &Lua) -> Result<()> {
    icefield.set(
        "from_json",
        lua.create_function(|lua, s: String| {
            let v = parse_json(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        })?,
    )?;

    icefield.set(
        "to_json",
        lua.create_function(|lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            Ok(serialize_json(&v))
        })?,
    )?;

    icefield.set(
        "from_toml",
        lua.create_function(|lua, s: String| {
            let v = parse_toml(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        })?,
    )?;

    icefield.set(
        "to_toml",
        lua.create_function(|lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            serialize_toml(&v)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;

    icefield.set(
        "from_yaml",
        lua.create_function(|lua, s: String| {
            let v = parse_yaml(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        })?,
    )?;

    icefield.set(
        "to_yaml",
        lua.create_function(|lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            serialize_yaml(&v)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;

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
pub fn parse_json(s: &str) -> anyhow::Result<serde_json::Value> {
    serde_json::from_str(s)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a pretty-printed JSON string.
pub fn serialize_json(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Parses a TOML string into a `serde_json::Value`.
pub fn parse_toml(s: &str) -> anyhow::Result<serde_json::Value> {
    toml::from_str(s).map_err(|e| anyhow::anyhow!("TOML parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a TOML string.
pub fn serialize_toml(v: &serde_json::Value) -> anyhow::Result<String> {
    toml::to_string(v)
        .map_err(|e| anyhow::anyhow!("TOML serialize error: {}", e))
}

/// Parses a YAML string into a `serde_json::Value`.
pub fn parse_yaml(s: &str) -> anyhow::Result<serde_json::Value> {
    serde_yaml::from_str(s)
        .map_err(|e| anyhow::anyhow!("YAML parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a YAML string.
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
/// Returns an error if the command fails to execute or returns a non-zero
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
    fn test_yaml_helpers() {
        let yaml = "foo: bar";
        let v = parse_yaml(yaml).unwrap();
        assert_eq!(v["foo"], "bar");
        assert_eq!(serialize_yaml(&v).unwrap().trim(), "foo: bar");
    }
}
