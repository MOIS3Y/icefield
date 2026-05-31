//! High-level Utility Library.
//!
//! This module registers helpers in the `icefield.lib` table, providing
//! generic data manipulation, hashing utilities, and extending standard Lua modules.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use mlua::{Lua, Result, Table};

/// Registers utility functions in the `icefield.lib` table.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let lib = lua.create_table()?;

    registry.register_func(
        &lib,
        lua,
        LuaApiItem {
            table: "lib",
            name: "fake_hash",
            description: "Returns a dummy hash for testing purposes.",
            example: None,
            kind: LuaItemKind::Function {
                params: &[],
                returns: "string",
            },
        },
        |_, ()| Ok("00000000000000000000000000000000"),
    )?;

    icefield.set("lib", lib)?;
    Ok(())
}
