//! Icefield Lua Standard Library.
//!
//! This module provides a set of helper functions and system information
//! exposed to the user's Lua configuration via the global `icefield` table.
//! It includes tools for path expansion, command execution, formatting,
//! and system inspection, organized into logical sub-tables.

pub mod color;
pub mod drv;
pub mod engine;
pub mod fetch;
pub mod format;
pub mod fs;
pub mod lib;
pub mod log;
pub mod prelude;
pub mod registry;
pub mod render;
pub mod sys;

use crate::paths::AppPaths;
use mlua::{Lua, Result};

/// Main entry point for preparing the Lua environment with Icefield's API.
///
/// Registers the `icefield` global table and its structured sub-tables.
pub fn register(
    lua: &Lua,
    paths: &AppPaths,
    registry: &mut registry::ApiRegistry,
) -> Result<()> {
    let icefield = lua.create_table()?;

    color::register(&icefield, lua, registry)?;
    drv::register(&icefield, lua, registry)?;
    fetch::register(&icefield, lua, paths, registry)?;
    format::register(&icefield, lua, registry)?;
    fs::register(&icefield, lua, paths, registry)?;
    lib::register(&icefield, lua, registry)?;
    log::register(&icefield, lua, registry)?;
    render::register(&icefield, lua, registry)?;
    sys::register(&icefield, lua, paths, registry)?;

    // Register metadata for injected pure-Lua functions
    prelude::register(registry)?;

    lua.globals().set("icefield", icefield)?;

    // Inject pure-Lua helpers into the global environment
    prelude::bootstrap(lua)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lua_hierarchy() -> Result<()> {
        let lua = Lua::new();
        let dir = tempfile::tempdir()
            .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
        let config_dir = dir.path().join("cfg");
        let paths = AppPaths::resolve(Some(config_dir));
        let mut reg = registry::ApiRegistry::new();
        register(&lua, &paths, &mut reg)?;

        // Check sys
        let os: String = lua.load("icefield.sys.os").eval()?;
        assert!(os == "linux" || os == "macos" || os == "unix");

        // Check format
        let json: String =
            lua.load("icefield.format.to_json({a=1})").eval()?;
        assert!(json.contains("\"a\": 1"));

        // Check drv
        let drv_type: String =
            lua.load("icefield.drv.mkText({}).type").eval()?;
        assert_eq!(drv_type, "text");

        // Check prelude (string.trim)
        let trimmed: String = lua.load("return ('  hi  '):trim()").eval()?;
        assert_eq!(trimmed, "hi");

        // Check parse_palette
        let bg_hex: String = lua
            .load(
                r##"
            local theme = icefield.color.parse_palette({
                bg = "#1e1e2e",
                not_color = "hello"
            })
            return theme.bg:to_hex()
        "##,
            )
            .eval()?;
        assert_eq!(bg_hex, "#1e1e2e");

        Ok(())
    }
}
