//! Phase 1: Compute.
//!
//! This module encapsulates the Lua runtime and is responsible for executing
//! the user's configuration and transforming it into a list of Rust-native
//! `Derivation` structures.
//!
//! It provides the `LuaEngine`, which prepares the Lua environment, registers
//! the Icefield API, and handles the deserialization of Lua tables into
//! Rust structures.

use crate::model::Derivation;
use anyhow::{Context, anyhow};
use mlua::{Lua, LuaSerdeExt, Result, Table, Value};

/// Engine for executing Lua configurations and generating Derivations.
///
/// The `LuaEngine` encapsulates the Lua runtime and provides the necessary
/// global functions (constructors) to the user's configuration script.
/// It maps Lua tables back to Rust `Derivation` structures using `serde`.
pub struct LuaEngine {
    lua: Lua,
}

impl LuaEngine {
    /// Creates a new `LuaEngine` and prepares the Lua environment.
    ///
    /// This method:
    /// 1. Initializes a new Lua state.
    /// 2. Injects the `CONFIG_DIR` global variable.
    /// 3. Configures `package.path` to allow `require` to find local modules.
    /// 4. Registers the system API and derivation constructors.
    ///
    /// # Errors
    ///
    /// Returns a Lua error if the initialization or API registration fails.
    pub fn new(
        config_dir: &std::path::Path,
        cache_dir: &std::path::Path,
    ) -> Result<Self> {
        let lua = mlua::Lua::new();

        let config_dir_str = config_dir.to_string_lossy().to_string();
        lua.globals().set("CONFIG_DIR", config_dir_str.clone())?;

        // Add CONFIG_DIR to package.path so `require` can find local modules
        let package: Table = lua.globals().get("package")?;
        let mut path: String = package.get("path")?;
        path.push_str(&format!(
            ";{}/?.lua;{}/?/init.lua",
            config_dir_str, config_dir_str
        ));
        package.set("path", path)?;

        crate::lua_api::register(&lua, config_dir, cache_dir)?;

        Ok(Self { lua })
    }

    /// Executes a Lua script and returns a list of Derivations.
    ///
    /// The script is expected to return a Lua table (array) of derivations.
    /// Derivations with `enable = false` are filtered out.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The Lua script has syntax errors.
    /// - The script execution fails.
    /// - The returned value cannot be deserialized into `Vec<Derivation>`.
    pub fn execute(
        &self,
        source: &str,
        chunk_name: &str,
    ) -> Result<Vec<Derivation>> {
        tracing::debug!(
            "Executing Lua configuration ({} bytes)",
            source.len()
        );
        // Load the script with the provided chunk name so `debug.getinfo` can
        // accurately determine the source file path.
        let value: Value =
            self.lua.load(source).set_name(chunk_name).eval()?;
        let mut derivations: Vec<Derivation> = self.lua.from_value(value)?;

        derivations.retain(|d| d.meta.enable);

        tracing::debug!(
            "Extracted {} active derivations from Lua",
            derivations.len()
        );
        Ok(derivations)
    }

    /// Loads and executes a Lua configuration from a file.
    ///
    /// This is a high-level helper that:
    /// 1. Reads the file content.
    /// 2. Determines the configuration root directory.
    /// 3. Initializes a new `LuaEngine`.
    /// 4. Executes Phase 1 (Compute).
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, has syntax errors,
    /// or fails to produce a valid list of derivations.
    pub fn load_file(
        path: &std::path::Path,
        cache_dir: &std::path::Path,
    ) -> anyhow::Result<Vec<Derivation>> {
        if !path.exists() {
            anyhow::bail!("Config file not found: {:?}", path);
        }

        let source = std::fs::read_to_string(path).with_context(|| {
            format!("Failed to read config file: {:?}", path)
        })?;

        let config_dir =
            path.parent().unwrap_or_else(|| std::path::Path::new("."));

        let engine = Self::new(config_dir, cache_dir)
            .map_err(|e| anyhow!("Failed to initialize Lua engine: {}", e))?;

        engine
            .execute(&source, &path.to_string_lossy())
            .map_err(|e| anyhow!("Lua execution failed: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_load_file_success() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let config_path = dir.path().join("init.lua");
        let mut file = std::fs::File::create(&config_path)?;

        writeln!(
            file,
            "{}",
            r#"
            local drv = icefield.drv
            return {
                drv.toml({
                    name = "test",
                    enable = true,
                    target = "out.toml",
                    source = {}
                })
            }
            "#
        )?;

        let derivations = LuaEngine::load_file(&config_path, dir.path())?;
        assert_eq!(derivations.len(), 1);
        assert_eq!(derivations[0].meta.name, "test");
        Ok(())
    }

    #[test]
    fn test_load_file_not_found() {
        let result = LuaEngine::load_file(
            std::path::Path::new("non_existent.lua"),
            std::path::Path::new("/dummy"),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_execute_simple_config() -> Result<()> {
        let engine = LuaEngine::new(
            std::path::Path::new("/dummy/config"),
            std::path::Path::new("/dummy/cache"),
        )
        .unwrap();
        let script = r#"
            local drv = icefield.drv
            return {
                drv.toml({
                    name = "test-toml",
                    enable = true,
                    target = "dummy/test.toml",
                    source = { key = "value" }
                })
            }
        "#;

        let derivations = engine.execute(script, "dummy/config/init.lua")?;
        assert_eq!(derivations.len(), 1);
        assert_eq!(derivations[0].meta.name, "test-toml");
        assert_eq!(
            derivations[0].meta.target.to_str().unwrap(),
            "dummy/test.toml"
        );
        Ok(())
    }

    #[test]
    fn test_execute_multiple_derivations() -> Result<()> {
        let engine = LuaEngine::new(
            std::path::Path::new("/dummy/config"),
            std::path::Path::new("/dummy/cache"),
        )
        .unwrap();
        let script = r#"
            local drv = icefield.drv
            return {
                drv.toml({
                    name = "toml",
                    enable = true,
                    target = "dummy/toml",
                    source = {}
                }),
                drv.symlink({
                    name = "link",
                    enable = true,
                    target = "dummy/link",
                    source_path = "/src/path"
                })
            }
        "#;

        let derivations = engine.execute(script, "dummy/config/init.lua")?;
        assert_eq!(derivations.len(), 2);
        assert_eq!(derivations[0].meta.name, "toml");
        assert_eq!(derivations[1].meta.name, "link");
        Ok(())
    }

    #[test]
    fn test_execute_invalid_script() {
        let engine = LuaEngine::new(
            std::path::Path::new("/dummy/config"),
            std::path::Path::new("/dummy/cache"),
        )
        .unwrap();
        let script = "this is not valid lua";
        let result = engine.execute(script, "dummy/config/init.lua");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_missing_required_fields() {
        let engine = LuaEngine::new(
            std::path::Path::new("/dummy/config"),
            std::path::Path::new("/dummy/cache"),
        )
        .unwrap();
        // Missing 'target' field
        let script = r#"
            local drv = icefield.drv
            return {
                drv.toml({
                    name = "invalid",
                    source = {}
                })
            }
        "#;
        let result = engine.execute(script, "dummy/config/init.lua");
        assert!(result.is_err());
    }
}
