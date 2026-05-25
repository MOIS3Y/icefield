{
  pkgs ? import <nixpkgs> { },
  self ? { },
  extraLuaPackages ? [ ],
  extraPackages ? [ ],
  allowCModules ? false,
}:

let
  lib = pkgs.lib;

  # fallback chain for git revision
  rev = self.shortRev or self.dirtyShortRev or "";
  revSuffix = lib.optionalString (rev != "") "+${rev}";

  # override lua package
  luaEnv = pkgs.lua5_4.withPackages (p: extraLuaPackages);

  # package meta
  cargoToml = fromTOML (builtins.readFile ./Cargo.toml);
  baseVersion = cargoToml.package.version;
  pname = cargoToml.package.name;
  version = "${baseVersion}${revSuffix}";
in
pkgs.rustPlatform.buildRustPackage (finalAttrs: {
  inherit pname version;

  src = ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  buildFeatures = lib.optional allowCModules "unsafe-c-modules";

  nativeBuildInputs = with pkgs; [
    pkg-config
    makeWrapper
  ];

  buildInputs = [ luaEnv ];

  postInstall = ''
    LUA_PATH_STR=$(${luaEnv}/bin/lua -e 'print(package.path)')
    LUA_CPATH_STR=$(${luaEnv}/bin/lua -e 'print(package.cpath)')
    wrapProgram $out/bin/icefield \
      --set LUA_PATH "$LUA_PATH_STR" \
      --set LUA_CPATH "$LUA_CPATH_STR" \
      --prefix PATH : ${lib.makeBinPath extraPackages}
  '';

  meta = with lib; {
    description = cargoToml.package.description;
    homepage = cargoToml.package.repository;
    license = licenses.mit;
    mainProgram = cargoToml.package.name;
  };
})
