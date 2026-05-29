//! Derivation Constructors.
//!
//! This module registers the base Lua functions (`mkText`, `mkCopy`, `mkLink`)
//! that the user calls to generate derivations in Phase 1. These are available
//! under the `icefield.drv` table.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use mlua::{Lua, Result, Table};

/// Registers the derivation constructors into the `icefield.drv` table.
///
/// These constructors add a `"type"` tag to the configuration table,
/// allowing Rust to deserialize it into the correct `DerivationKind`.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let drv = lua.create_table()?;

    registry.register_func(
        &drv,
        lua,
        LuaApiItem {
            table: "drv",
            name: "mkText",
            description: "Constructs a new managed 'text' derivation.",
            example: Some(
                r##"
                icefield.drv.mkText({
                  name = "script",
                  enable = true,
                  dst = "~/bin/hello",
                  src = "#!/bin/sh\necho Hello",
                  mode = "755"
                })
                "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "table",
            },
        },
        |_, args: Table| {
            args.set("type", "text")?;
            let name: String =
                args.get("name").unwrap_or_else(|_| "unnamed".to_string());
            let dst: String =
                args.get("dst").unwrap_or_else(|_| "unknown".to_string());
            tracing::debug!("Created 'text' derivation: {} -> {}", name, dst);
            Ok(args)
        },
    )?;

    registry.register_func(
        &drv,
        lua,
        LuaApiItem {
            table: "drv",
            name: "mkCopy",
            description: "Constructs a new managed 'copy' derivation.",
            example: Some(
                r#"
                icefield.drv.mkCopy({
                  name = "wallpaper",
                  enable = true,
                  dst = "~/Pictures/bg.jpg",
                  src = icefield.fs.config_dir() .. "/files/wall.jpg"
                })
                "#,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "table",
            },
        },
        |_, args: Table| {
            args.set("type", "copy")?;
            let name: String =
                args.get("name").unwrap_or_else(|_| "unnamed".to_string());
            let dst: String =
                args.get("dst").unwrap_or_else(|_| "unknown".to_string());
            tracing::debug!("Created 'copy' derivation: {} -> {}", name, dst);
            Ok(args)
        },
    )?;

    registry.register_func(
        &drv,
        lua,
        LuaApiItem {
            table: "drv",
            name: "mkLink",
            description: "Constructs a new managed 'symlink' derivation.",
            example: Some(
                r#"
                icefield.drv.mkLink({
                  name = "scripts",
                  enable = true,
                  dst = "~/bin/tool",
                  src = "/absolute/path/to/tool"
                })
                "#,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "table",
            },
        },
        |_, args: Table| {
            args.set("type", "symlink")?;
            let name: String =
                args.get("name").unwrap_or_else(|_| "unnamed".to_string());
            let dst: String =
                args.get("dst").unwrap_or_else(|_| "unknown".to_string());
            tracing::debug!(
                "Created 'symlink' derivation: {} -> {}",
                name,
                dst
            );
            Ok(args)
        },
    )?;

    icefield.set("drv", drv)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::engine::LuaEngine;
    use crate::paths::AppPaths;
    use tempfile::tempdir;

    #[test]
    fn test_execute_simple_config() -> Result<()> {
        let dir = tempdir().unwrap();
        let paths = AppPaths::resolve(Some(dir.path().to_path_buf()));
        let engine = LuaEngine::new(&paths).unwrap();
        let script = r#"
            return {
                icefield.drv.mkText({
                    name = "test-toml",
                    enable = true,
                    dst = "dummy/test.toml",
                    src = "value"
                })
            }
        "#;

        let derivations = engine.execute(script, "init.lua")?;
        assert_eq!(derivations.len(), 1);
        assert_eq!(derivations[0].meta.name, "test-toml");
        assert_eq!(
            derivations[0].meta.dst.to_str().unwrap(),
            "dummy/test.toml"
        );
        Ok(())
    }

    #[test]
    fn test_execute_multiple_derivations() -> Result<()> {
        let dir = tempdir().unwrap();
        let paths = AppPaths::resolve(Some(dir.path().to_path_buf()));
        let engine = LuaEngine::new(&paths).unwrap();
        let script = r#"
            return {
                icefield.drv.mkText({
                    name = "toml",
                    enable = true,
                    dst = "dummy/toml",
                    src = ""
                }),
                icefield.drv.mkLink({
                    name = "link",
                    enable = true,
                    dst = "dummy/link",
                    src = "/src/path"
                })
            }
        "#;

        let derivations = engine.execute(script, "init.lua")?;
        assert_eq!(derivations.len(), 2);
        assert_eq!(derivations[0].meta.name, "toml");
        assert_eq!(derivations[1].meta.name, "link");
        Ok(())
    }

    #[test]
    fn test_execute_missing_required_fields() -> Result<()> {
        let dir = tempdir().unwrap();
        let paths = AppPaths::resolve(Some(dir.path().to_path_buf()));
        let engine = LuaEngine::new(&paths).unwrap();
        // Missing 'dst' field
        let script = r#"
            return {
                icefield.drv.mkText({
                    name = "invalid",
                    src = ""
                })
            }
        "#;
        let result = engine.execute(script, "init.lua");
        assert!(result.is_err());
        Ok(())
    }
}
