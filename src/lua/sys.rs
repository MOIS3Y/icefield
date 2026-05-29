//! System Information and Command Execution.
//!
//! This module registers the `icefield.sys` table, giving the Lua environment
//! access to OS details, hostname, username, and functions to spawn external commands.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use crate::paths::AppPaths;
use mlua::{Lua, Result, Table};
use std::path::Path;

/// Registers system functions and variables in the `icefield.sys` table.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    paths: &AppPaths,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let sys = lua.create_table()?;

    registry.register_var(
        &sys,
        LuaApiItem {
            table: "sys",
            name: "os",
            description: "The name of the current operating system.",
            example: None,
            kind: LuaItemKind::Variable {
                type_name: "string",
            },
        },
        get_os(),
    )?;
    registry.register_var(
        &sys,
        LuaApiItem {
            table: "sys",
            name: "username",
            description: "The name of the currently logged-in user.",
            example: None,
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
            description: "The network hostname of the current machine.",
            example: None,
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
            description: "Checks if a command-line tool is available in the system PATH.",
            example: Some(r##"
                if icefield.sys.has_command("git") then
                    print("Git is installed")
                end
            "##),
            kind: LuaItemKind::Function {
                params: &[("cmd", "string")],
                returns: "boolean",
            },
        },
        |_, cmd: String| Ok(has_command(&cmd)),
    )?;

    {
        let run_cmd_dir = paths.config_dir.clone();
        registry.register_func(
            &sys,
            lua,
            LuaApiItem {
                table: "sys",
                name: "spawn",
                description: "Executes an external command and returns its standard output.",
                example: Some(r##"
                    -- String syntax
                    local out = icefield.sys.spawn("date")

                    -- Table syntax (safer for arguments)
                    local out = icefield.sys.spawn({ "ls", "-la", "~" })
                "##),
                kind: LuaItemKind::Function {
                    params: &[("cmd_or_args", "string|table")],
                    returns: "string",
                },
            },
            move |_, value: mlua::Value| {
                let (cmd, args) = match value {
                    mlua::Value::String(s) => {
                        (s.to_str()?.to_string(), vec![])
                    }
                    mlua::Value::Table(t) => {
                        let cmd: String = t.get(1)?;
                        let args: Vec<String> = t
                            .sequence_values::<String>()
                            .skip(1)
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        (cmd, args)
                    }
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "spawn expects a string or a table".into(),
                        ));
                    }
                };
                run_command(&cmd, args, &run_cmd_dir)
            },
        )?;
    }

    {
        let run_cmd_dir = paths.config_dir.clone();
        registry.register_func(
            &sys,
            lua,
            LuaApiItem {
                table: "sys",
                name: "spawn_sh",
                description: "Executes a command string via the system shell (sh).",
                example: Some(r##"
                    local out = icefield.sys.spawn_sh("ls -la ~ | grep ssh")
                "##),
                kind: LuaItemKind::Function {
                    params: &[("cmd_line", "string")],
                    returns: "string",
                },
            },
            move |_, cmd_line: String| {
                run_command(
                    "sh",
                    vec!["-c".to_string(), cmd_line],
                    &run_cmd_dir,
                )
            },
        )?;
    }

    icefield.set("sys", sys)?;
    Ok(())
}

/// Returns the name of the operating system.
pub fn get_os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unix"
    }
}

/// Returns the current user's name.
pub fn get_username() -> String {
    whoami::username().unwrap_or_else(|_| "unknown".into())
}

/// Returns the system's hostname.
pub fn get_hostname() -> String {
    whoami::hostname().unwrap_or_else(|_| "unknown".into())
}

/// Returns true if the given command is available in the system PATH.
pub fn has_command(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

/// Executes an external command and returns its standard output.
///
/// # Errors
///
/// Returns an error if the command fails to start or exits with a non-zero code.
pub fn run_command(
    cmd: &str,
    args: Vec<String>,
    dir: &Path,
) -> Result<String> {
    use console::style;
    tracing::debug!("Executing system command: {} {}", cmd, args.join(" "));
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
                let stdout =
                    String::from_utf8_lossy(&output.stdout).into_owned();
                tracing::trace!("Command output: {}", stdout);
                Ok(stdout)
            } else {
                let err_msg = format!(
                    "Command failed with exit code {}: {}",
                    output.status.code().unwrap_or(-1),
                    cmd
                );
                tracing::error!("{}", err_msg);
                Err(mlua::Error::RuntimeError(err_msg))
            }
        }
        Err(e) => {
            tracing::error!("Failed to execute command: {}", e);
            Err(mlua::Error::RuntimeError(format!(
                "Failed to execute command: {}",
                e
            )))
        }
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
    fn test_has_command() {
        // standard commands that exist everywhere
        assert!(has_command("ls") || has_command("sh"));
        assert!(!has_command("non-existent-command-xyz-123"));
    }
}
