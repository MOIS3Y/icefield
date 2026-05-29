# Icefield ❄️

Icefield is a declarative dotfile manager powered by Rust and Lua. It
allows you to manage your system configuration files with the power of a
real programming language while maintaining an atomic and predictable
system state.

> [!IMPORTANT]
> This project is currently under active development. While functional, use
> with caution on production systems.

## Features

- **2-Phase Atomic Model**: Compute the entire system state in memory
  before writing a single byte to the disk.
- **Structured Lua API**: Explicit organization into `drv`, `fs`, `sys`,
  `fetch`, `format`, and `render` modules.
- **Smart Templating**: Native support for Tera (Jinja2) and SCSS with
  variable injection and recursive imports.
- **Content-Addressable Store**: Remote fetchers for URL, GitHub, GitLab,
  and Gitea with SHA-256 integrity verification.
- **Atomic Commits**: Changes are applied atomically using temporary files
  and renames to prevent partial updates.
- **Privilege Elevation**: Native support for `sudo`/`doas` for managing
  system-wide configuration files.
- **XDG Compliance**: Respects standard directories for configuration
  (`~/.config/icefield`), data, and state.
- **IDE Support**: Generate EmmyLua stubs for full autocompletion and type
  checking in your editor.

## How it Works

Icefield simplifies system management into two primary phases:

1.  **Compute & Render (Phase 1)**: Your `init.lua` is executed. High-level
    logic, template rendering, and data serialization (JSON, TOML, etc.)
    happen in memory. The result is a flat list of "Derivations"
    representing the desired state.
2.  **Commit (Phase 2)**: The computed state is compared with the current
    system state (`state.json`). Only changed files are written, and files
    no longer present in the configuration are safely removed (Garbage
    Collection).

## Installation

Icefield requires Rust and Lua 5.4. It is best used within a Nix
environment:

```bash
# Enter the development shell
nix develop

# Or build the binary
cargo build --release
```

## Usage

Define your system state in `~/.config/icefield/init.lua`:

```lua
-- Icefield Configuration Example
local drv = icefield.drv
local fs = icefield.fs
local fmt = icefield.format
local rnd = icefield.render
local fetch = icefield.fetch

return {
  -- Managed TOML configuration with explicit formatting
  drv.mkText({
    name = "helix-config",
    enable = true,
    dst = fs.expand("~/.config/helix/config.toml"),
    src = fmt.to_toml({
      theme = "catppuccin_mocha",
      editor = {
        line_numbers = "relative",
        cursor_shape = { insert = "bar" }
      }
    })
  }),

  -- Dynamic SCSS styling with template variables
  drv.mkText({
    name = "waybar-style",
    enable = true,
    dst = fs.expand("~/.config/waybar/style.css"),
    src = rnd.scss({
      src = fs.config_dir() .. "/waybar/main.scss",
      vars = { accent = "#ff5500", bg = "#1e1e2e" }
    })
  }),

  -- Remote artifacts with integrity checks
  drv.mkCopy({
    name = "wallpaper",
    enable = true,
    dst = fs.expand("~/Pictures/bg.jpg"),
    src = fetch.url({
      url = "https://example.com/wallpaper.jpg",
      hash = "sha256-..."
    })
  }),

  -- Scripts with execution permissions
  drv.mkText({
    name = "hello-script",
    enable = true,
    dst = "~/bin/hello",
    src = "#!/bin/sh\necho 'Hello from Icefield!'",
    mode = "755"
  })
}
```

### CLI Commands

- `icefield switch`: Compute and apply the configuration.
- `icefield info`: Show discrepancies between the config and the filesystem.
- `icefield stubs`: Generate Lua API stubs for your IDE.

## License

This project is licensed under the MIT License.
