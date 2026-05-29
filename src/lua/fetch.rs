//! Remote Artifact Fetchers.
//!
//! This module registers functions in the `icefield.fetch` table, providing
//! the ability to download remote files, archives, and repositories securely
//! through content-addressable storage mechanisms.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use crate::paths::AppPaths;
use crate::store::Store;
use mlua::{Lua, Result, Table};

/// Registers fetcher functions in the `icefield.fetch` table.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    paths: &AppPaths,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let fetch = lua.create_table()?;
    let sd = paths.store_dir();

    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "url",
            description: "Downloads a file from a URL.",
            example: Some(
                r##"
                local path = icefield.fetch.url({
                    url = "https://example.com/file.tar.gz",
                    hash = "sha256-...",
                    name = "my-file" -- optional
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "string",
            },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            tracing::debug!("Fetching URL: {} (hash: {})", url, hash);
            let path = store
                .fetch_url(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "URL"))?;
            tracing::debug!("Artifact stored at: {}", path.display());
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "tarball",
            description: "Downloads and extracts a tarball.",
            example: Some(
                r##"
                local path = icefield.fetch.tarball({
                    url = "https://github.com/user/repo/archive/main.tar.gz",
                    hash = "sha256-...",
                    name = "my-repo" -- optional
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "string",
            },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            tracing::debug!("Fetching tarball: {} (hash: {})", url, hash);
            let path = store
                .fetch_tarball(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "tarball"))?;
            tracing::debug!("Artifact extracted to: {}", path.display());
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "zip",
            description: "Downloads and extracts a ZIP archive.",
            example: Some(
                r##"
                local path = icefield.fetch.zip({
                    url = "https://example.com/archive.zip",
                    hash = "sha256-..."
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "string",
            },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let url: String = args.get("url")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            tracing::debug!("Fetching ZIP: {} (hash: {})", url, hash);
            let path = store
                .fetch_zip(&url, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "ZIP"))?;
            tracing::debug!("Artifact extracted to: {}", path.display());
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "from_github",
            description: "Fetches from GitHub.",
            example: Some(
                r##"
                local path = icefield.fetch.from_github({
                    owner = "rust-lang",
                    repo = "rust",
                    rev = "master",
                    hash = "sha256-..."
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "string",
            },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let host: Option<String> = args.get("host")?;
            let owner: String = args.get("owner")?;
            let repo: String = args.get("repo")?;
            let rev: String = args.get("rev")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            tracing::debug!(
                "Fetching from GitHub: {}/{}@{} (hash: {})",
                owner,
                repo,
                rev,
                hash
            );
            let path = store
                .fetch_from_github(host, &owner, &repo, &rev, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "GitHub"))?;
            tracing::debug!("Artifact stored at: {}", path.display());
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "from_gitlab",
            description: "Fetches from GitLab.",
            example: Some(
                r##"
                local path = icefield.fetch.from_gitlab({
                    owner = "inkscape",
                    repo = "inkscape",
                    rev = "master",
                    hash = "sha256-..."
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "string",
            },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let host: Option<String> = args.get("host")?;
            let owner: String = args.get("owner")?;
            let repo: String = args.get("repo")?;
            let rev: String = args.get("rev")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            tracing::debug!(
                "Fetching from GitLab: {}/{}@{} (hash: {})",
                owner,
                repo,
                rev,
                hash
            );
            let path = store
                .fetch_from_gitlab(host, &owner, &repo, &rev, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "GitLab"))?;
            tracing::debug!("Artifact stored at: {}", path.display());
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    let s = sd.clone();
    registry.register_func(
        &fetch,
        lua,
        LuaApiItem {
            table: "fetch",
            name: "from_gitea",
            description: "Fetches from Gitea.",
            example: Some(
                r##"
                local path = icefield.fetch.from_gitea({
                    host = "gitea.com",
                    owner = "gitea",
                    repo = "tea",
                    rev = "main",
                    hash = "sha256-..."
                })
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[("args", "table")],
                returns: "string",
            },
        },
        move |_, args: Table| {
            let store = Store::new(&s);
            let host: Option<String> = args.get("host")?;
            let owner: String = args.get("owner")?;
            let repo: String = args.get("repo")?;
            let rev: String = args.get("rev")?;
            let hash: String = args.get("hash")?;
            let name: Option<String> = args.get("name")?;
            tracing::debug!(
                "Fetching from Gitea: {}/{}@{} (hash: {})",
                owner,
                repo,
                rev,
                hash
            );
            let path = store
                .fetch_from_gitea(host, &owner, &repo, &rev, &hash, name)
                .map_err(|e| wrap_fetch_err(e, "Gitea"))?;
            tracing::debug!("Artifact stored at: {}", path.display());
            Ok(path.to_string_lossy().into_owned())
        },
    )?;

    icefield.set("fetch", fetch)?;
    Ok(())
}

fn wrap_fetch_err(e: anyhow::Error, kind: &str) -> mlua::Error {
    mlua::Error::RuntimeError(format!("\nFetch failed ({}): {}", kind, e))
}
