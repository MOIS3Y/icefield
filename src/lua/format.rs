//! Data Serialization and Parsing.
//!
//! This module registers helpers in the `icefield.format` table to parse and
//! serialize structured data (JSON, TOML, YAML, INI, ENV) directly from Lua.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use anyhow::Context;
use mlua::{Lua, LuaSerdeExt, Result, Table};
use std::collections::BTreeMap;

/// Registers data serialization and parsing helpers in the `icefield.format` table.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let format = lua.create_table()?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "from_json",
            description: "Parses a JSON string into a Lua table.",
            example: Some(
                r##"
                local data = icefield.format.from_json('{"foo": "bar"}')
                print(data.foo) -- "bar"
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("s", "string")],
                returns: "table",
            },
        },
        |lua, s: String| {
            let v = from_json(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "to_json",
            description: "Serializes a Lua table to a pretty-printed JSON string.",
            example: Some(r##"
                local json = icefield.format.to_json({
                    foo = "bar",
                    nested = { 1, 2, 3 }
                })
            "##),
            kind: LuaItemKind::Function {
                params: &[("t", "table")],
                returns: "string",
            },
        },
        |lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            Ok(to_json(&v))
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "from_toml",
            description: "Parses a TOML string into a Lua table.",
            example: Some(
                r##"
                local data = icefield.format.from_toml('foo = "bar"')
                print(data.foo) -- "bar"
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("s", "string")],
                returns: "table",
            },
        },
        |lua, s: String| {
            let v = from_toml(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "to_toml",
            description: "Serializes a Lua table to a TOML string.",
            example: Some(
                r##"
                local toml = icefield.format.to_toml({
                    editor = { line_numbers = "relative" }
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("t", "table")],
                returns: "string",
            },
        },
        |lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            to_toml(&v).map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "from_yaml",
            description: "Parses a YAML string into a Lua table.",
            example: Some(
                r##"
                local data = icefield.format.from_yaml("foo: bar")
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("s", "string")],
                returns: "table",
            },
        },
        |lua, s: String| {
            let v = from_yaml(&s)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            lua.to_value(&v)
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "to_yaml",
            description: "Serializes a Lua table to a YAML string.",
            example: Some(
                r##"
                local yaml = icefield.format.to_yaml({ foo = "bar" })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("t", "table")],
                returns: "string",
            },
        },
        |lua, t: mlua::Value| {
            let v: serde_json::Value = lua.from_value(t)?;
            to_yaml(&v).map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "to_ini",
            description: "Serializes a nested Lua table to an INI string.",
            example: Some(
                r##"
                local ini = icefield.format.to_ini({
                    Section = { key = "value" }
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("t", "table")],
                returns: "string",
            },
        },
        |lua, t: mlua::Value| {
            let v: BTreeMap<String, BTreeMap<String, String>> =
                lua.from_value(t)?;
            to_ini(&v).map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        },
    )?;

    registry.register_func(
        &format,
        lua,
        LuaApiItem {
            table: "format",
            name: "to_env",
            description: "Serializes a flat Lua table to a .env formatted string.",
            example: Some(r##"
                local env = icefield.format.to_env({
                    API_KEY = "secret",
                    DEBUG = "true"
                })
            "##),
            kind: LuaItemKind::Function {
                params: &[("t", "table")],
                returns: "string",
            },
        },
        |lua, t: mlua::Value| {
            let v: BTreeMap<String, String> = lua.from_value(t)?;
            Ok(to_env(&v))
        },
    )?;

    icefield.set("format", format)?;
    Ok(())
}

/// Parses a JSON string into a `serde_json::Value`.
fn from_json(s: &str) -> anyhow::Result<serde_json::Value> {
    serde_json::from_str(s)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a pretty-printed JSON string.
fn to_json(v: &serde_json::Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Parses a TOML string into a `serde_json::Value`.
fn from_toml(s: &str) -> anyhow::Result<serde_json::Value> {
    toml::from_str(s).map_err(|e| anyhow::anyhow!("TOML parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a TOML string.
fn to_toml(v: &serde_json::Value) -> anyhow::Result<String> {
    toml::to_string(v)
        .map_err(|e| anyhow::anyhow!("TOML serialize error: {}", e))
}

/// Parses a YAML string into a `serde_json::Value`.
fn from_yaml(s: &str) -> anyhow::Result<serde_json::Value> {
    serde_yaml::from_str(s)
        .map_err(|e| anyhow::anyhow!("YAML parse error: {}", e))
}

/// Serializes a `serde_json::Value` into a YAML string.
fn to_yaml(v: &serde_json::Value) -> anyhow::Result<String> {
    serde_yaml::to_string(v)
        .map_err(|e| anyhow::anyhow!("YAML serialize error: {}", e))
}

/// Generates the content for a flat `.env` file.
///
/// Keys and values are separated by `=`, and values are always enclosed in double quotes.
fn to_env(source: &BTreeMap<String, String>) -> String {
    let mut content = String::new();
    for (k, v) in source {
        content.push_str(&format!("{}=\"{}\"\n", k, v));
    }
    content
}

/// Generates the content for an INI file.
///
/// Expects a nested map where the first level represents sections, and the
/// second level represents key-value pairs within that section.
fn to_ini(
    source: &BTreeMap<String, BTreeMap<String, String>>,
) -> anyhow::Result<String> {
    let mut ini = ini::Ini::new();
    for (section, params) in source {
        for (k, v) in params {
            ini.with_section(Some(section)).set(k, v);
        }
    }
    let mut buffer = Vec::new();
    ini.write_to(&mut buffer).context("Failed to write INI")?;
    String::from_utf8(buffer).context("INI output is not UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_conversions() {
        let v = json!({"foo": "bar"});
        let s = to_json(&v);
        assert!(s.contains("\"foo\": \"bar\""));
        let v2 = from_json(&s).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn test_toml_conversions() {
        let v = json!({"foo": "bar"});
        let s = to_toml(&v).unwrap();
        assert_eq!(s.trim(), "foo = \"bar\"");
        let v2 = from_toml(&s).unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn test_env() {
        let mut map = BTreeMap::new();
        map.insert("A".to_string(), "val2".to_string());
        map.insert("Z".to_string(), "val1".to_string());
        assert_eq!(to_env(&map), "A=\"val2\"\nZ=\"val1\"\n");
    }

    #[test]
    fn test_ini() {
        let mut map = BTreeMap::new();
        let mut sec = BTreeMap::new();
        sec.insert("key".to_string(), "val".to_string());
        map.insert("section".to_string(), sec);
        let ini = to_ini(&map).unwrap();
        assert!(ini.contains("[section]"));
        assert!(ini.contains("key=val"));
    }
}
