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
    /// This includes functions like `mkTomlDerivation`, `mkSymlinkDerivation`,
    /// etc., which the user calls in their `init.lua`.
    pub fn new() -> Result<Self> {
        let lua = Lua::new();
        Self::register_constructors(&lua)?;
        Ok(Self { lua })
    }

    /// Registers `mk*Derivation` functions in the Lua global scope.
    ///
    /// Each registered function takes a table, injects a `"type"` field
    /// corresponding to the `DerivationKind`, and returns the table back.
    fn register_constructors(lua: &Lua) -> Result<()> {
        let globals = lua.globals();

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
            let func = lua.create_function(move |_, args: Table| {
                args.set("type", kind_tag)?;
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
    pub fn execute(&self, source: &str) -> Result<Vec<Derivation>> {
        tracing::debug!(
            "Executing Lua configuration ({} bytes)",
            source.len()
        );
        let value: Value = self.lua.load(source).eval()?;
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
        let engine = LuaEngine::new()?;
        let script = r#"
            return {
                mkTomlDerivation({
                    name = "test-toml",
                    target = "dummy/test.toml",
                    source = { key = "value" }
                })
            }
        "#;

        let derivations = engine.execute(script)?;
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
        let engine = LuaEngine::new()?;
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

        let derivations = engine.execute(script)?;
        assert_eq!(derivations.len(), 2);
        assert_eq!(derivations[0].meta.name, "toml");
        assert_eq!(derivations[1].meta.name, "link");
        Ok(())
    }

    #[test]
    fn test_execute_invalid_script() {
        let engine = LuaEngine::new().unwrap();
        let script = "this is not valid lua";
        let result = engine.execute(script);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_missing_required_fields() {
        let engine = LuaEngine::new().unwrap();
        // Missing 'target' field
        let script = r#"
            return {
                mkTomlDerivation({
                    name = "invalid",
                    source = {}
                })
            }
        "#;
        let result = engine.execute(script);
        assert!(result.is_err());
    }
}
