# Icefield

Icefield is a declarative dotfile manager powered by Rust and Lua. It allows you to manage your system configuration files with the power of a real programming language while maintaining an atomic and predictable state.

> [!IMPORTANT]
> This project is currently under active development (Work In Progress). Use with caution on production systems.

## Features

- **Hierarchical Lua API**: Clean organization into `drv`, `fs`, `sys`, `fetch`, and `format` sub-tables.
- **Atomic Commits**: Changes are staged and applied atomically using temporary files and renames.
- **XDG Compliance**: Respects system standards for configuration, data, and state directories.
- **Content-Addressable Store**: Remote fetchers for URL, GitHub, GitLab, and Gitea with SHA-256 verification (Nix-style Base32).
- **Drift Detection**: Use `icefield info` to see discrepancies between your config and the filesystem.
- **Privilege Elevation**: Native support for `sudo`/`doas` when writing to system paths.
- **Flexible Formats**: First-class support for JSON, YAML, TOML, INI, and environment variables.
- **Developer Friendly**: Overridable paths via `ICEFIELD_*` environment variables and automatic development shell setup via Nix Flake.

## How it Works

Icefield operates in three distinct phases:

1.  **Compute**: Your `init.lua` is executed to generate a high-level graph of "derivations" (the desired state).
2.  **Build**: Templates are rendered and data structures are serialized in memory. No files are written to the system yet.
3.  **Commit**: The built content is compared with the current system state. Only changed files are updated, and orphaned files are removed.

## Installation

> [!WARNING]
> Icefield currently requires a Nix-based environment or manual installation of Lua 5.4 development headers.

If you are using Nix, you can enter the development environment directly:

```bash
nix develop
```

## Usage

Define your system state in `init.lua` (usually at `~/.config/icefield/init.lua`):

```lua
local drv = icefield.drv
local fs = icefield.fs
local fetch = icefield.fetch

return {
  -- Managed TOML configuration
  drv.toml({
    name = "helix-config",
    enable = true,
    target = fs.expand("~/.config/helix/config.toml"),
    source = {
      theme = "catppuccin_mocha",
      editor = { line_numbers = "relative" }
    }
  }),

  -- Remote artifacts with integrity checks
  drv.copy({
    name = "wallpaper",
    enable = true,
    target = fs.expand("~/Pictures/bg.jpg"),
    source_path = fetch.url({
      url = "https://example.com/wall.jpg",
      hash = "18ayigi9i1hn461vdy082v6balrwgg58brfgkqd6984w0qxd86xp"
    })
  }),

  -- Symbolic links
  drv.symlink({
    name = "scripts",
    enable = true,
    target = fs.expand("~/bin/my-tool"),
    source_path = fs.config_dir() .. "/files/my-tool.sh"
  }),

  -- Scripts with execution permissions
  drv.text({
    name = "hello-script",
    enable = true,
    target = "~/bin/hello",
    source = "#!/bin/sh\necho 'Hello from Icefield!'",
    mode = "755" -- Set executable flag
  })
}
```

Apply changes:

```bash
icefield switch
```

## License

This project is licensed under the MIT License.
