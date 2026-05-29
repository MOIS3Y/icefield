//! Lua Prelude and Environment Bootstrapping.
//!
//! This module handles the injection of pure Lua helpers and extensions to the
//! standard Lua library (e.g., `string.trim`). It ensures that the Lua environment
//! has a consistent set of utility functions available regardless of the
//! configuration script.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use mlua::{Lua, Result};

/// Registers metadata for functions injected via the prelude into the registry.
///
/// This ensures that even though these functions are implemented in Lua,
/// they still appear in the generated EmmyLua stubs.
pub fn register(registry: &mut ApiRegistry) -> Result<()> {
    registry.items.push(LuaApiItem {
        table: "lib.string",
        name: "trim",
        description: "Removes leading and trailing whitespace from a string.",
        example: Some(
            r##"
            local s = "  hello  "
            print(s:trim()) -- "hello"
        "##,
        ),
        kind: LuaItemKind::Function {
            params: &[("s", "string")],
            returns: "string",
        },
    });

    Ok(())
}

/// Injects pure Lua helpers and extensions into the global environment.
///
/// This is called during the Lua engine initialization to set up the
/// "prelude" of available functions.
pub fn bootstrap(lua: &Lua) -> Result<()> {
    tracing::debug!("Bootstrapping Lua environment (prelude)");

    lua.load(
        r#"
        -- Patch global string table
        function string.trim(s)
            return s:match("^%s*(.-)%s*$")
        end

        -- Ensure icefield.lib.string mapping exists
        local lib = icefield.lib
        lib.string = lib.string or {}
        lib.string.trim = string.trim
        "#,
    )
    .exec()
}
