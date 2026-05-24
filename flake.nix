{
  description = "icefield - declarative dotfile manager powered by Rust and Lua";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      # self,
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
            gcc
            pkg-config
            lua5_4
          ];

          # Environment variables for easy development/testing
          ICEFIELD_CONFIG_DIR = "./.test_env/config";
          ICEFIELD_DATA_DIR = "./.test_env/data";
          ICEFIELD_CACHE_DIR = "./.test_env/cache";
          ICEFIELD_STATE_DIR = "./.test_env/state";

          shellHook = ''
            echo -e "❄ \e[1;34mIcefield Development Shell\e[0m"
            echo -e "Environment variables set to point to \e[1;33m./.test_env\e[0m"

            # Ensure directories exist
            mkdir -p .test_env/{config,data,cache,state,out}
          '';
        };
      }
    );
}
