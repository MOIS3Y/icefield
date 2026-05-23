use crate::model::Derivation;
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
    /// Creates a new `LuaEngine` and initializes global constructors.
    ///
    /// The `config_dir` is used to resolve relative paths (like `source_path`
    /// or `template_path`) relative to the location of the `init.lua` file,
    /// rather than the user's current working directory.
    pub fn new(config_dir: &std::path::Path) -> Result<Self> {
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

        Self::register_constructors(&lua)?;
        Ok(Self { lua })
    }

    /// Registers `mk*Derivation` functions in the Lua global scope.
    ///
    /// Each registered function takes a table, injects a `"type"` field
    /// corresponding to the `DerivationKind`, and returns the table back.
    /// It also resolves relative `template_path` and `source_path` against `CONFIG_DIR`.
    fn register_constructors(lua: &Lua) -> Result<()> {
        let globals = lua.globals();

        // Helper Lua function to resolve relative paths against the root config directory.
        let resolve_path_script = r#"
            function resolve_path(path)
                -- If it's already an absolute path or relative to home, leave it alone
                if type(path) == "string" and path:sub(1, 1) ~= "/" and path:sub(1, 2) ~= "~/" then
                    return CONFIG_DIR .. "/" .. path
                end
                return path
            end
        "#;
        lua.load(resolve_path_script).exec()?;

        let kinds = [
            ("mkTomlDerivation", "toml"),
            ("mkYamlDerivation", "yaml"),
            ("mkJsonDerivation", "json"),
            ("mkEnvDerivation", "env"),
            ("mkIniDerivation", "ini"),
            ("mkSymlinkDerivation", "symlink"),
            ("mkScssDerivation", "scss"),
            ("mkTemplateDerivation", "template"),
        ];

        for (func_name, kind_tag) in kinds {
            let func = lua.create_function(move |lua, args: Table| {
                args.set("type", kind_tag)?;

                let resolve_path: mlua::Function =
                    lua.globals().get("resolve_path")?;

                if let Ok(template_path) =
                    args.get::<mlua::Value>("template_path")
                {
                    args.set(
                        "template_path",
                        resolve_path.call::<mlua::Value>(template_path)?,
                    )?;
                }

                if let Ok(source_path) = args.get::<mlua::Value>("source_path")
                {
                    args.set(
                        "source_path",
                        resolve_path.call::<mlua::Value>(source_path)?,
                    )?;
                }

                Ok(args)
            })?;
            globals.set(func_name, func)?;
        }

        Ok(())
    }

    /// Executes a Lua script and returns a list of Derivations.
    ///
    /// The script is expected to return a Lua table (array) of derivations.
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
        let derivations: Vec<Derivation> = self.lua.from_value(value)?;
        tracing::debug!(
            "Extracted {} derivations from Lua",
            derivations.len()
        );
        Ok(derivations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_simple_config() -> Result<()> {
        let engine = LuaEngine::new(std::path::Path::new("/dummy/config"))?;
        let script = r#"
            return {
                mkTomlDerivation({
                    name = "test-toml",
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
        let engine = LuaEngine::new(std::path::Path::new("/dummy/config"))?;
        let script = r#"
            return {
                mkTomlDerivation({
                    name = "toml",
                    target = "dummy/toml",
                    source = {}
                }),
                mkSymlinkDerivation({
                    name = "link",
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
        let engine =
            LuaEngine::new(std::path::Path::new("/dummy/config")).unwrap();
        let script = "this is not valid lua";
        let result = engine.execute(script, "dummy/config/init.lua");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_missing_required_fields() {
        let engine =
            LuaEngine::new(std::path::Path::new("/dummy/config")).unwrap();
        // Missing 'target' field
        let script = r#"
            return {
                mkTomlDerivation({
                    name = "invalid",
                    source = {}
                })
            }
        "#;
        let result = engine.execute(script, "dummy/config/init.lua");
        assert!(result.is_err());
    }
}
