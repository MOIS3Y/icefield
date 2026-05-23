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
        Self::register_icefield_api(&lua, config_dir)?;
        Self::register_lua_helpers(&lua)?;

        Ok(Self { lua })
    }

    /// Registers the `icefield` global table and its functions.
    fn register_icefield_api(
        lua: &Lua,
        config_dir: &std::path::Path,
    ) -> Result<()> {
        let icefield = lua.create_table()?;
        let config_dir = config_dir.to_path_buf();

        // icefield.run_command(cmd, args)
        // Runs an external command, captures stdout, and inherits stdin/stderr.
        let run_command = lua.create_function(
            move |_, (cmd, args): (String, Vec<String>)| {
                use console::style;

                println!(
                    "  {} {} {}",
                    style("➜").blue(),
                    style("Running:").dim(),
                    style(format!("{} {}", cmd, args.join(" "))).italic()
                );

                let result = duct::cmd(cmd.clone(), args)
                    .dir(&config_dir)
                    .stdout_capture()
                    .unchecked()
                    .run();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            let stdout =
                                String::from_utf8_lossy(&output.stdout)
                                    .into_owned();
                            Ok(stdout)
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
            },
        )?;

        icefield.set("run_command", run_command)?;
        lua.globals().set("icefield", icefield)?;
        Ok(())
    }

    /// Registers useful Lua helper functions (like string trimming).
    fn register_lua_helpers(lua: &Lua) -> Result<()> {
        // Add :trim() method to all strings
        let trim_script = r#"
            function string.trim(s)
                return s:match("^%s*(.-)%s*$")
            end
        "#;
        lua.load(trim_script).exec()?;
        Ok(())
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
    ) -> anyhow::Result<Vec<Derivation>> {
        if !path.exists() {
            anyhow::bail!("Config file not found: {:?}", path);
        }

        let source = std::fs::read_to_string(path).with_context(|| {
            format!("Failed to read config file: {:?}", path)
        })?;

        let config_dir =
            path.parent().unwrap_or_else(|| std::path::Path::new("."));

        let engine = Self::new(config_dir)
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
            r#"
            return {{
                mkTomlDerivation({{
                    name = "test",
                    target = "out.toml",
                    source = {{}}
                }})
            }}
            "#
        )?;

        let derivations = LuaEngine::load_file(&config_path)?;
        assert_eq!(derivations.len(), 1);
        assert_eq!(derivations[0].meta.name, "test");
        Ok(())
    }

    #[test]
    fn test_load_file_not_found() {
        let result =
            LuaEngine::load_file(std::path::Path::new("non_existent.lua"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_execute_simple_config() -> Result<()> {
        let engine =
            LuaEngine::new(std::path::Path::new("/dummy/config")).unwrap();
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
        let engine =
            LuaEngine::new(std::path::Path::new("/dummy/config")).unwrap();
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
