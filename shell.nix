{
  pkgs ? import <nixpkgs> { },
}:

let
  package = pkgs.callPackage ./default.nix { };

  # dev tools
  ice-update-stubs = pkgs.writeShellScriptBin "ice-update-stubs" ''
    echo "Updating Lua stubs..."
    mkdir -p meta
    cargo run --quiet -- stubs > meta/icefield.lua
    echo "Stubs updated in meta/icefield.lua"
  '';

  ice-init = pkgs.writeShellScriptBin "ice-init" ''
    echo "Initializing development environment..."

    # Ensure directories exist
    mkdir -p .test_env/{config,data,cache,state,out}
    mkdir -p meta

    # Auto-generate .luarc.json for Lua LSP if it doesn't exist
    if [ ! -f .luarc.json ]; then
      cat > .luarc.json << EOF
      {
        "workspace": {
          "library": ["meta"],
          "ignoreDir": ["meta"]
        },
        "diagnostics": {
          "globals": ["icefield"],
          "disable": ["duplicate-set-field"]
        }
      }
    EOF
      echo -e "Created \e[1;33m.luarc.json\e[0m for Lua LSP."
    else
      echo ".luarc.json already exists."
    fi
      echo "Environment initialized successfully."
  '';
in
pkgs.mkShell {
  inputsFrom = [ package ];

  buildInputs = with pkgs; [
    # rust
    cargo
    clippy
    rustfmt
    rust-analyzer

    # dev tools
    ice-update-stubs
    ice-init
  ];

  RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

  # Environment variables for easy development/testing
  ICEFIELD_CONFIG_DIR = "./.test_env/config";
  ICEFIELD_DATA_DIR = "./.test_env/data";
  ICEFIELD_CACHE_DIR = "./.test_env/cache";
  ICEFIELD_STATE_DIR = "./.test_env/state";

  shellHook = ''
    echo -e "❄ \e[1;34mIcefield Development Shell\e[0m"
    echo -e "Environment variables set to point to \e[1;33m./.test_env\e[0m"
    echo -e "Tip: Run \e[1;32mice-init\e[0m to scaffold the test directories and IDE config."
    echo -e "Tip: Run \e[1;32mice-update-stubs\e[0m to regenerate Lua IDE definitions."
  '';
}
