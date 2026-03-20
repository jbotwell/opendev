{
  description = "OpenDev — AI-powered command-line tool for accelerated development";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        craneLib = crane.mkLib pkgs;

        rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        pinnedSrc = builtins.fetchGit {
          url = "https://github.com/jbotwell/opendev";
          rev = "f6eddbccb4d55148c8ac754a4a82db2616dfd0bc";
        };

        src = pkgs.lib.cleanSourceWith {
          src = pinnedSrc;
          filter = path: type:
            (pkgs.lib.hasInfix "/templates/" path) ||
            (pkgs.lib.hasInfix "/skills/" path) ||
            (craneLib.filterCargoSources path type);
        };

        commonArgs = {
          inherit src;
          strictDeps = true;
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
          doCheck = false;
        });

        opendev = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname = "opendev";
          cargoExtraArgs = "--package opendev-cli";
          doCheck = false;
        });

      in
      {
        packages.default = opendev;
        packages.opendev = opendev;

        apps.default = flake-utils.lib.mkApp {
          drv = opendev;
          name = "opendev";
        };

        devShells.default = craneLib.devShell {
          inputsFrom = [ opendev ];
          packages = with pkgs; [
            rustToolchain
            rust-analyzer
            cargo-watch
            cargo-edit
            bacon
            nodejs
            typescript
            tailwindcss-language-server
          ];
        };
      }
    );
}
