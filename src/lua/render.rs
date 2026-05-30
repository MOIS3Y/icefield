//! Template and Style Rendering.
//!
//! This module provides the `icefield.render.*` functions to allow the user
//! to dynamically render templates and compile SCSS into CSS using context
//! variables during the Lua execution phase.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use anyhow::Context;
use mlua::{Lua, LuaSerdeExt, Result, Table};
use std::fs;
use std::path::{Path, PathBuf};

/// Registers the rendering functions in the `icefield.render` table.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let render = lua.create_table()?;

    registry.register_func(
        &render,
        lua,
        LuaApiItem {
            table: "render",
            name: "template",
            description: "Renders a Tera template using a config table.",
            example: Some(
                r##"
                local rendered = icefield.render.template({
                    src = icefield.fs.config_dir() .. "/templates/config.j2",
                    vars = { user = "admin" },
                    scope = icefield.fs.config_dir() .. "/templates",
                    includes = { "/path/to/extra.j2" }
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "string",
            },
        },
        |lua, args: Table| {
            let path: String = args.get("src")?;
            tracing::debug!("Rendering template: {}", path);

            let vars: serde_json::Value = args
                .get::<mlua::Value>("vars")
                .ok()
                .map(|v| lua.from_value(v))
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            tracing::trace!("Template variables: {:?}", vars);

            let mut includes = None;
            if let Ok(inc_list) = args.get::<Vec<String>>("includes") {
                includes = Some(
                    inc_list
                        .into_iter()
                        .map(PathBuf::from)
                        .collect::<Vec<_>>(),
                );
            }

            let mut scope = None;
            if let Ok(s) = args.get::<String>("scope") {
                scope = Some(PathBuf::from(s));
            }

            let result = render_template(
                Path::new(&path),
                scope.as_deref(),
                includes.as_deref(),
                &vars,
            )
            .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;

            tracing::debug!(
                "Template rendered successfully ({} bytes)",
                result.len()
            );
            Ok(result)
        },
    )?;

    registry.register_func(
        &render,
        lua,
        LuaApiItem {
            table: "render",
            name: "scss",
            description: "Compiles SCSS to CSS using a configuration table.",
            example: Some(
                r##"
                local css = icefield.render.scss({
                    src = icefield.fs.config_dir() .. "/styles/main.scss",
                    vars = { accent = "#ff5500" },
                    scope = icefield.fs.config_dir() .. "/styles",
                    includes = { "/other/styles" }
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "string",
            },
        },
        |lua, args: Table| {
            let path: String = args.get("src")?;
            tracing::debug!("Compiling SCSS: {}", path);

            let vars: serde_json::Value = args
                .get::<mlua::Value>("vars")
                .ok()
                .map(|v| lua.from_value(v))
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            tracing::trace!("SCSS template variables: {:?}", vars);

            let mut includes = None;
            if let Ok(inc_list) = args.get::<Vec<String>>("includes") {
                includes = Some(
                    inc_list
                        .into_iter()
                        .map(PathBuf::from)
                        .collect::<Vec<_>>(),
                );
            }

            let mut scope = None;
            if let Ok(s) = args.get::<String>("scope") {
                scope = Some(PathBuf::from(s));
            }

            let result = render_scss(
                Path::new(&path),
                scope.as_deref(),
                includes.as_deref(),
                &vars,
            )
            .map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;

            tracing::debug!(
                "SCSS compiled successfully ({} bytes)",
                result.len()
            );
            Ok(result)
        },
    )?;

    icefield.set("render", render)?;
    Ok(())
}

/// Renders a template file using Tera with the provided variables.
///
/// # Errors
///
/// Returns an error if the template cannot be read, parsed, or rendered.
fn render_template(
    path: &Path,
    scope: Option<&Path>,
    includes: Option<&[PathBuf]>,
    variables: &serde_json::Value,
) -> anyhow::Result<String> {
    let mut tera = tera::Tera::default();

    let effective_scope = resolve_effective_scope(path, scope)?;
    load_scope_templates(&mut tera, &effective_scope)?;

    if let Some(inc_paths) = includes {
        load_extra_templates(&mut tera, inc_paths)?;
    }

    // Ensure the main template is loaded (might be outside scope or overridden)
    let template_content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read template: {:?}", path))?;
    tera.add_raw_template("main", &template_content)?;

    let context = tera::Context::from_value(variables.clone())?;
    tera.render("main", &context)
        .map_err(|e| anyhow::anyhow!("Template rendering failed: {}", e))
}

/// Determines the effective base directory for template discovery.
fn resolve_effective_scope(
    path: &Path,
    scope: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let effective = scope
        .map(|s| s.to_path_buf())
        .or_else(|| path.parent().map(|p| p.to_path_buf()))
        .ok_or_else(|| {
            anyhow::anyhow!("Could not determine template scope")
        })?;

    Ok(fs::canonicalize(&effective).unwrap_or(effective))
}

/// Discovers and registers all templates within the specified scope.
fn load_scope_templates(
    tera: &mut tera::Tera,
    scope: &Path,
) -> anyhow::Result<()> {
    if !scope.is_dir() {
        return Ok(());
    }

    tracing::trace!("Tera: loading templates from scope: {}", scope.display());
    let glob_pattern = format!("{}/**/*", scope.to_string_lossy());

    let entries = glob::glob(&glob_pattern)
        .map_err(|e| anyhow::anyhow!("Invalid glob pattern: {}", e))?;

    for entry in entries.flatten() {
        if entry.is_file() {
            register_single_template(tera, scope, &entry);
        }
    }
    Ok(())
}

/// Registers a single template file with Tera, using a clean relative name.
fn register_single_template(tera: &mut tera::Tera, scope: &Path, path: &Path) {
    let mut name = path
        .strip_prefix(scope)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();

    if name.starts_with('/') {
        name = name[1..].to_string();
    }

    if let Ok(content) = fs::read_to_string(path) {
        if let Err(e) = tera.add_raw_template(&name, &content) {
            tracing::trace!("Tera: skipping {} (parse error: {})", name, e);
        } else {
            tracing::trace!("Tera: registered template: {}", name);
        }
    }
}

/// Registers extra template files provided explicitly.
fn load_extra_templates(
    tera: &mut tera::Tera,
    includes: &[PathBuf],
) -> anyhow::Result<()> {
    for inc_path in includes {
        let name = inc_path.file_name().and_then(|n| n.to_str()).ok_or_else(
            || anyhow::anyhow!("Invalid include path: {:?}", inc_path),
        )?;

        tracing::trace!(
            "Adding explicit template include: {} as {}",
            inc_path.display(),
            name
        );
        tera.add_template_file(inc_path, Some(name))
            .with_context(|| {
                format!("Failed to include template: {:?}", inc_path)
            })?;
    }
    Ok(())
}

/// A virtual file system implementation for `grass` (SCSS compiler) that processes
/// imported files through `tera` (template engine) before passing them to the compiler.
#[derive(Debug)]
struct TeraFs {
    /// The template context containing the variables to be injected.
    context: tera::Context,
}

impl grass::Fs for TeraFs {
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        tracing::trace!("TeraFs: reading and rendering {}", path.display());
        let content = fs::read_to_string(path)?;
        let rendered = tera::Tera::one_off(&content, &self.context, false)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Tera render error: {}", e),
                )
            })?;
        Ok(rendered.into_bytes())
    }
}

/// Compiles an SCSS file to CSS after processing it as a Tera template.
///
/// # Errors
///
/// Returns an error if rendering the template or compiling SCSS fails.
fn render_scss(
    path: &Path,
    scope: Option<&Path>,
    includes: Option<&[PathBuf]>,
    variables: &serde_json::Value,
) -> anyhow::Result<String> {
    let context = tera::Context::from_value(variables.clone())?;
    let fs = TeraFs { context };

    let mut options = grass::Options::default().fs(&fs);

    if let Some(s) = scope {
        options = options.load_path(s);
    } else if let Some(parent) = path.parent() {
        options = options.load_path(parent);
    }

    if let Some(incs) = includes {
        for inc in incs {
            options = options.load_path(inc);
        }
    }

    grass::from_path(path, &options)
        .map_err(|e| anyhow::anyhow!("SCSS compilation failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn test_render_template() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let subdir = dir.path().join("templates");
        fs::create_dir(&subdir)?;

        let file_path = subdir.join("test.j2");
        // Tera uses double curly braces for variables: {{ var }}
        write!(fs::File::create(&file_path)?, "Hello, {{{{ user_name }}}}")?;

        let variables = json!({ "user_name": "World" });
        let content = render_template(&file_path, None, None, &variables)?;
        assert_eq!(content, "Hello, World");
        Ok(())
    }

    #[test]
    fn test_render_scss() -> anyhow::Result<()> {
        let mut file = tempfile::NamedTempFile::new()?;
        // Tera: {{ color }}, SCSS literal: { }
        write!(file, "$color: {{{{ color }}}}; body {{ color: $color; }}")?;

        let variables = json!({ "color": "red" });
        let content = render_scss(file.path(), None, None, &variables)?;
        assert!(content.contains("color: red"));
        Ok(())
    }
}
