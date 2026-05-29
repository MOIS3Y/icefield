//! Filesystem Utilities.
//!
//! This module registers the `icefield.fs` table, providing path expansion,
//! existence checks, directory listing, and file reading capabilities to the Lua environment.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use crate::paths::AppPaths;
use anyhow::Context;
use mlua::{Lua, Result, Table};
use std::path::Path;

/// Registers filesystem functions in the `icefield.fs` table.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    paths: &AppPaths,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let fs = lua.create_table()?;

    let cfg_dir = paths.config_dir.clone();
    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "config_dir",
            description: "Returns the absolute path to the configuration directory.",
            example: Some(r##"
                local cfg = icefield.fs.config_dir()
            "##),
            kind: LuaItemKind::Function {
                params: &[],
                returns: "string",
            },
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
            description: "Returns the absolute path to the application cache directory.",
            example: Some(r##"
                local cache = icefield.fs.cache_dir()
            "##),
            kind: LuaItemKind::Function {
                params: &[],
                returns: "string",
            },
        },
        move |_, ()| Ok(cch_dir.to_string_lossy().into_owned()),
    )?;

    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "home_dir",
            description: "Returns the absolute path to the user's home directory.",
            example: Some(r##"
                local home = icefield.fs.home_dir()
            "##),
            kind: LuaItemKind::Function {
                params: &[],
                returns: "string",
            },
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
            example: Some(r##"
                if icefield.fs.exists("~/.bashrc") then
                    print("Found bashrc")
                end
            "##),
            kind: LuaItemKind::Function {
                params: &[("path", "string")],
                returns: "boolean",
            },
        },
        |_, path: String| {
            tracing::trace!("Checking existence: {}", path);
            Ok(path_exists(&path))
        },
    )?;

    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "expand",
            description: "Expands tildes (~) and environment variables in the given path.",
            example: Some(r##"
                local full_path = icefield.fs.expand("~/$USER/config")
            "##),
            kind: LuaItemKind::Function {
                params: &[("path", "string")],
                returns: "string",
            },
        },
        |_, path: String| {
            let expanded = path_expand(&path);
            tracing::trace!("Expanded path: {} -> {}", path, expanded);
            Ok(expanded)
        },
    )?;

    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "ls",
            description: "Lists the contents of a directory, returning a table of entries.",
            example: Some(r##"
                local files = icefield.fs.ls("~/.config")
                for _, file in ipairs(files) do
                    print(file.name, file.type)
                end
            "##),
            kind: LuaItemKind::Function {
                params: &[("path", "string")],
                returns: "table",
            },
        },
        |lua, path: String| {
            tracing::trace!("Listing directory: {}", path);
            let entries = fs_ls(&path).map_err(|e| {
                mlua::Error::RuntimeError(format!("ls failed: {}", e))
            })?;
            let result = lua.create_table()?;
            for (i, (name, type_str)) in entries.into_iter().enumerate() {
                let entry_table = lua.create_table()?;
                entry_table.set("name", name)?;
                entry_table.set("type", type_str)?;
                result.set(i + 1, entry_table)?;
            }
            Ok(result)
        },
    )?;

    registry.register_func(
        &fs,
        lua,
        LuaApiItem {
            table: "fs",
            name: "read_file",
            description: "Reads the entire content of a file and returns it as a string.",
            example: Some(r##"
                local content = icefield.fs.read_file("~/.bashrc")
            "##),
            kind: LuaItemKind::Function {
                params: &[("path", "string")],
                returns: "string",
            },
        },
        |_, path: String| {
            tracing::trace!("Reading file: {}", path);
            fs_read_file(&path).map_err(|e| {
                mlua::Error::RuntimeError(format!("read_file failed: {}", e))
            })
        },
    )?;

    icefield.set("fs", fs)?;
    Ok(())
}

/// Returns the user's home directory path.
///
/// Falls back to `/` if the home directory cannot be determined.
pub fn get_home_dir() -> String {
    directories::UserDirs::new()
        .map(|u| u.home_dir().to_string_lossy().into_owned())
        .unwrap_or_else(|| "/".into())
}

/// Lists the contents of a directory.
///
/// Returns a list of tuples containing (filename, type_string) where
/// type_string is "file", "directory", or "symlink".
///
/// # Errors
///
/// Returns an error if the path cannot be read.
pub fn fs_ls(path: &str) -> anyhow::Result<Vec<(String, String)>> {
    let expanded = path_expand(path);
    let dir_path = Path::new(&expanded);
    let entries = std::fs::read_dir(dir_path).with_context(|| {
        format!("Failed to read directory: {:?}", dir_path)
    })?;
    let mut result = Vec::new();
    for entry in entries {
        let entry: std::fs::DirEntry = entry?;
        let file_type = entry.file_type()?;
        let type_str = if file_type.is_dir() {
            "directory"
        } else if file_type.is_symlink() {
            "symlink"
        } else {
            "file"
        };
        result.push((
            entry.file_name().to_string_lossy().into_owned(),
            type_str.to_string(),
        ));
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// Reads the entire content of a file as a string.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn fs_read_file(path: &str) -> anyhow::Result<String> {
    let expanded = path_expand(path);
    std::fs::read_to_string(&expanded)
        .with_context(|| format!("Failed to read file: {:?}", expanded))
}

/// Checks if a path exists after expanding tildes and environment variables.
pub fn path_exists(path: &str) -> bool {
    Path::new(&path_expand(path)).exists()
}

/// Expands tildes (`~`) and environment variables in a path string.
pub fn path_expand(path: &str) -> String {
    shellexpand::full(path)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_path_expand() {
        let expanded = path_expand("~/test");
        // Must contain absolute root on unix or windows
        assert!(expanded.starts_with('/') || expanded.contains(":\\"));
    }

    #[test]
    fn test_fs_operations() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world")?;

        let sub_dir = dir.path().join("subdir");
        fs::create_dir(&sub_dir)?;

        assert!(path_exists(file_path.to_str().unwrap()));
        assert_eq!(fs_read_file(file_path.to_str().unwrap())?, "hello world");

        let entries = fs_ls(dir.path().to_str().unwrap())?;
        assert_eq!(entries.len(), 2);

        // Sorting ensures deterministic order
        assert_eq!(entries[0].0, "subdir");
        assert_eq!(entries[0].1, "directory");
        assert_eq!(entries[1].0, "test.txt");
        assert_eq!(entries[1].1, "file");

        Ok(())
    }
}
