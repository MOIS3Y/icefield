//! Icefield Lua Standard Library.
//!
//! This module provides a set of helper functions and system information
//! exposed to the user's Lua configuration via the global `icefield` table.
//! It includes tools for path expansion, command execution, and system inspection.

use mlua::{Lua, Result};
use std::path::Path;

/// Registers the `icefield` global table and all its helper functions.
pub fn register(lua: &Lua, config_dir: &Path) -> Result<()> {
    let icefield = lua.create_table()?;

    // 1. System Info (as static values)
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

    // 4. icefield.run_command(cmd, args)
    let run_cmd_dir = config_dir.to_path_buf();
    icefield.set(
        "run_command",
        lua.create_function(move |_, (cmd, args): (String, Vec<String>)| {
            run_command(&cmd, args, &run_cmd_dir)
        })?,
    )?;

    lua.globals().set("icefield", icefield)?;

    // Register string.trim helper
    lua.load(
        r#"
        function string.trim(s)
            return s:match("^%s*(.-)%s*$")
        end
    "#,
    )
    .exec()?;

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
}
