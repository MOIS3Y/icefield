# Icefield

Icefield is a declarative dotfile manager powered by Rust and Lua. It allows you to manage your system configuration files with the power of a real programming language while maintaining an atomic and predictable state.

> [!IMPORTANT]
> This project is currently under active development (Work In Progress). Use with caution on production systems.

## Key Features

*   **Declarative Configuration**: Define your desired system state in Lua.
*   **Atomic Updates**: Changes are applied in phases to prevent system inconsistency.
*   **Template Support**: Use Jinja2-style templates (via Tera) for dynamic configurations.
*   **Style Pre-processing**: Built-in SCSS compilation for managing complex stylesheets.
*   **Garbage Collection**: Automatically removes files that are no longer present in your configuration.
*   **State Tracking**: Maintains a `state.json` file to track managed files and their integrity via SHA-256 hashes.

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

## Quick Start

1. Create a configuration file at `~/.config/icefield/init.lua`.
2. Define your first derivation:
   ```lua
   return {
     mkTomlDerivation({
       name = "example-config",
       target = "~/.config/example/config.toml",
       source = {
         enabled = true,
         theme = "dark"
       }
     })
   }
   ```
3. Apply the configuration:
   ```bash
   icefield apply
   ```

## License

This project is licensed under the MIT License.
