{
  description = "TaskTrack is an simple app to manage personal tasks";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      treefmt-nix,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs {
          inherit system;
          overlays = overlays;
        };

        rust = pkgs.rust-bin.stable.latest.default;

        treefmt = treefmt-nix.lib.evalModule pkgs ./treefmt.nix;
      in
      {
        # Add a dev shell
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rust
            pkgs.rust-analyzer
            treefmt.config.package
            pkgs.podman-compose
            pkgs.atac # postman alternative in terminal
          ];

          shellHook = ''
            echo "🦀 Dev shell with rust + treefmt"
          '';
        };

        apps.treefmt = {
          type = "app";
          program = "${treefmt.config.package}/bin/treefmt";
        };

        formatter = treefmt.config.build.wrapper;
        checks = {
          formatting = treefmt.config.build.check self;
        };
      }
    );
}
