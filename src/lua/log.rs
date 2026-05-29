//! User Logging API.
//!
//! This module registers the `icefield.log` table, exposing the application's
//! internal `tracing` logger to the Lua environment. This allows users to inject
//! custom debug, info, warning, and error messages directly into the central
//! `icefield.log` file, facilitating easier debugging of complex configurations.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use mlua::{Lua, Result, Table};

/// Registers the logging functions in the `icefield.log` table.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let log = lua.create_table()?;

    // Macro for generating standard logging functions with identical metadata structure
    macro_rules! register_log_level {
        ($level_name:expr, $level_macro:path, $desc:expr) => {
            let example_str = format!(
                "icefield.log.{}(\"Configuration evaluated successfully.\")",
                $level_name
            );
            // Leak the string to get a &'static str required by LuaApiItem.
            // This is safe because register is only called once during app startup
            // and the memory overhead of 5 tiny strings is negligible.
            let example: &'static str = Box::leak(example_str.into_boxed_str());

            registry.register_func(
                &log,
                lua,
                LuaApiItem {
                    table: "log",
                    name: $level_name,
                    description: $desc,
                    example: Some(example),
                    kind: LuaItemKind::Function {
                        params: &[("message", "string")],
                        returns: "nil",
                    },
                },
                |_, msg: String| {
                    $level_macro!(target: "lua", "{}", msg);
                    Ok(())
                },
            )?;
        };
    }

    register_log_level!(
        "error",
        tracing::error,
        "Logs an error message to the central icefield.log file."
    );
    register_log_level!(
        "warn",
        tracing::warn,
        "Logs a warning message to the central icefield.log file."
    );
    register_log_level!(
        "info",
        tracing::info,
        "Logs an informational message to the central icefield.log file."
    );
    register_log_level!(
        "debug",
        tracing::debug,
        "Logs a debug message. Visible only when verbosity is increased (-v)."
    );
    register_log_level!(
        "trace",
        tracing::trace,
        "Logs a trace message. Visible only at maximum verbosity (-vv)."
    );

    icefield.set("log", log)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_execution() -> Result<()> {
        let lua = Lua::new();
        let mut registry = ApiRegistry::new();
        let icefield = lua.create_table()?;
        register(&icefield, &lua, &mut registry)?;
        lua.globals().set("icefield", icefield)?;

        // Test that calling the log functions doesn't crash the Lua engine.
        // We can't easily assert the output here without mocking the tracing subscriber,
        // but executing without errors is sufficient for API boundary testing.
        lua.load(
            r#"
            icefield.log.info("Test info message")
            icefield.log.debug("Test debug message")
            icefield.log.warn("Test warn message")
            icefield.log.error("Test error message")
            icefield.log.trace("Test trace message")
        "#,
        )
        .exec()?;

        Ok(())
    }
}
