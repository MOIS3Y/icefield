//! Phase 2: Build.
//!
//! This module implements the sandbox builder responsible for transforming
//! derivations into their final string representations (serialization,
//! template rendering, SCSS compilation). All build operations are performed
//! in memory to ensure atomicity.

use crate::model::{Derivation, DerivationKind};
use anyhow::{Context, Result, anyhow};

/// Sandbox builder for rendering derivations into final content.
///
/// The `Builder` is responsible for the "Phase 2: Build" of the application.
/// It takes a `Derivation` ( Phase 1 output) and transforms it into the
/// final string content (e.g., rendered TOML, compiled CSS, or processed
/// template). All operations are performed in memory.
pub struct Builder;

impl Builder {
    /// Dispatches the build process based on the `DerivationKind`.
    ///
    /// This is the main entry point for generating file content.
    ///
    /// # Errors
    ///
    /// Returns an error if any of the specialized build methods fail
    /// (e.g., template syntax error, SCSS compilation error).
    pub fn build(derivation: &Derivation) -> Result<String> {
        tracing::debug!("Building derivation: {}", derivation.meta.name);
        let content = match &derivation.kind {
            DerivationKind::Json { source } => {
                serde_json::to_string_pretty(source)
                    .context("Failed to serialize JSON")
            }
            DerivationKind::Yaml { source } => serde_yaml::to_string(source)
                .context("Failed to serialize YAML"),
            DerivationKind::Toml { source } => {
                toml::to_string(source).context("Failed to serialize TOML")
            }
            DerivationKind::Ini { source } => Self::build_ini(source),
            DerivationKind::Env { source } => Ok(Self::build_env(source)),
            DerivationKind::Text { source } => Ok(source.clone()),
            DerivationKind::Template {
                source,
                includes,
                variables,
            } => Self::build_template(source, includes.as_deref(), variables),
            DerivationKind::Scss {
                source,
                includes,
                variables,
            } => Self::build_scss(source, includes.as_deref(), variables),
            DerivationKind::Copy { .. } => Ok(String::new()),
            DerivationKind::Symlink { .. } => {
                Ok(String::new()) // Symlinks don't have content
            }
        }?;

        tracing::debug!(
            "Successfully built {}. Content length: {} bytes",
            derivation.meta.name,
            content.len()
        );
        Ok(content)
    }

    /// Generates the content for a flat `.env` file.
    ///
    /// The output is guaranteed to be deterministic (sorted by keys) to
    /// ensure consistent content hashing across executions.
    fn build_env(
        source: &std::collections::BTreeMap<String, String>,
    ) -> String {
        tracing::debug!("Building .env from {} variables", source.len());
        let mut content = String::new();
        for (k, v) in source {
            content.push_str(&format!("{}=\"{}\"\n", k, v));
        }
        content
    }

    /// Generates the content for an INI file.
    ///
    /// The output is guaranteed to be deterministic (sorted by sections and keys)
    /// to ensure consistent content hashing across executions.
    ///
    /// # Errors
    ///
    /// Returns an error if the INI structure cannot be serialized or
    /// if the resulting buffer is not valid UTF-8.
    fn build_ini(
        source: &std::collections::BTreeMap<
            String,
            std::collections::BTreeMap<String, String>,
        >,
    ) -> Result<String> {
        tracing::debug!("Building INI from {} sections", source.len());
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

    /// Renders a template using the `Tera` engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the template file cannot be read, contains
    /// syntax errors, or if the provided variables are invalid.
    fn build_template(
        path: &std::path::Path,
        includes: Option<&[std::path::PathBuf]>,
        variables: &serde_json::Value,
    ) -> Result<String> {
        tracing::debug!("Rendering template: {:?}", path);
        let mut tera = tera::Tera::default();

        if let Some(inc_paths) = includes {
            for inc_path in inc_paths {
                let name =
                    inc_path.file_name().and_then(|n| n.to_str()).ok_or_else(
                        || anyhow!("Invalid include path: {:?}", inc_path),
                    )?;

                tera.add_template_file(inc_path, Some(name)).with_context(
                    || format!("Failed to include template: {:?}", inc_path),
                )?;
            }
        }

        let template_content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read template: {:?}", path))?;

        tera.add_raw_template("main", &template_content)?;
        let context = tera::Context::from_value(variables.clone())?;
        tera.render("main", &context)
            .context("Template rendering failed")
    }

    /// Compiles SCSS to CSS after processing it as a template.
    ///
    /// This allows injecting dynamic variables (like theme colors) into
    /// stylesheets before they are compiled by the `grass` crate.
    ///
    /// # Errors
    ///
    /// Returns an error if template rendering fails or if the SCSS
    /// compilation encounters a syntax error.
    fn build_scss(
        path: &std::path::Path,
        includes: Option<&[std::path::PathBuf]>,
        variables: &serde_json::Value,
    ) -> Result<String> {
        tracing::debug!("Compiling SCSS from template: {:?}", path);
        // First, render SCSS as a template to inject variables
        let rendered_scss = Self::build_template(path, includes, variables)?;
        // Then, compile SCSS to CSS
        grass::from_string(rendered_scss, &grass::Options::default())
            .map_err(|e| anyhow!("SCSS compilation failed: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CommonMeta;
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn mock_meta() -> CommonMeta {
        CommonMeta {
            name: "test".to_string(),
            enable: true,
            target: PathBuf::from("dummy/path.txt"),
            force: None,
            sudo: None,
            owner: None,
            group: None,
            mode: None,
        }
    }

    #[test]
    fn test_build_toml() -> Result<()> {
        let der = Derivation {
            meta: mock_meta(),
            kind: DerivationKind::Toml {
                source: json!({ "foo": "bar" }),
            },
        };
        let content = Builder::build(&der)?;
        assert_eq!(content.trim(), "foo = \"bar\"");
        Ok(())
    }

    #[test]
    fn test_build_env() -> Result<()> {
        let mut source = std::collections::BTreeMap::new();
        source.insert("Z".to_string(), "val1".to_string());
        source.insert("A".to_string(), "val2".to_string());

        let content = Builder::build_env(&source);
        assert_eq!(content, "A=\"val2\"\nZ=\"val1\"\n");
        Ok(())
    }

    #[test]
    fn test_build_template() -> Result<()> {
        let mut file = NamedTempFile::new()?;
        use std::io::Write;
        // Tera uses double curly braces for variables: {{ var }}
        write!(file, "Hello, {{{{ user_name }}}}")?;

        let variables = json!({ "user_name": "World" });
        let content = Builder::build_template(file.path(), None, &variables)?;
        assert_eq!(content, "Hello, World");
        Ok(())
    }

    #[test]
    fn test_build_scss() -> Result<()> {
        let mut file = NamedTempFile::new()?;
        use std::io::Write;
        // Tera: {{ color }}, SCSS literal: { }
        write!(file, "$color: {{{{ color }}}}; body {{ color: $color; }}")?;

        let variables = json!({ "color": "red" });
        let content = Builder::build_scss(file.path(), None, &variables)?;
        assert!(content.contains("color: red"));
        Ok(())
    }

    #[test]
    fn test_build_text() -> Result<()> {
        let der = Derivation {
            meta: mock_meta(),
            kind: DerivationKind::Text {
                source: "hello text".to_string(),
            },
        };
        let content = Builder::build(&der)?;
        assert_eq!(content, "hello text");
        Ok(())
    }
}
