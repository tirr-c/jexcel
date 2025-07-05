{
  inputs = {
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "nixpkgs/nixos-unstable";
  };

  outputs =
    { fenix, flake-utils, nixpkgs, ... }:

    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        toolchainSpec = {
          channel = "1.88.0";
          sha256 = "sha256-Qxt8XAuaUR2OMdKbN4u8dBJOhSHxS+uS06Wl9+flVEk=";
        };
        fenix' = fenix.packages.${system};
        toolchain = fenix'.toolchainOf toolchainSpec;
        completeToolchain = fenix'.combine (with toolchain; [
          defaultToolchain
          rust-src
          rust-analyzer
        ]);
      in
      {
        devShell = pkgs.mkShell {
          name = "jexcel";
          packages = [
            completeToolchain
            pkgs.cmake
            pkgs.rustPlatform.bindgenHook
          ];
        };
      }
    );
}
