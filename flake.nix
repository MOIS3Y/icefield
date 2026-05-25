{
  description = "icefield - declarative dotfile manager powered by Rust and Lua";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
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
        packages.default = pkgs.callPackage ./default.nix {
          inherit self;
        };

        apps.default =
          flake-utils.lib.mkApp {
            drv = self.packages.${system}.default;
          }
          // {
            meta = self.packages.${system}.default.meta;
          };

        devShells.default = pkgs.callPackage ./shell.nix { };

        formatter = pkgs.nixfmt;
      }
    );
}
