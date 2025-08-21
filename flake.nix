{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    systems.url = "github:nix-systems/default";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs: inputs.flake-parts.lib.mkFlake { inherit inputs; } {
    systems = import inputs.systems;
    perSystem = { config, self', pkgs, lib, system, rust-overlay, ... }: rec {
      _module.args.pkgs = import inputs.nixpkgs {
        inherit system;
        overlays = [ (import inputs.rust-overlay) ];
      };

      packages.rust-toolchain = pkgs.rust-bin.fromRustupToolchainFile
        ./rust-toolchain.toml;

      # Rust dev environment
      devShells.default = pkgs.mkShell {
        shellHook = ''
            # For rust-analyzer 'hover' tooltips to work.
            export RUST_SRC_PATH=${packages.rust-toolchain.availableComponents.rust-src}
            export LIBCLANG_PATH=${pkgs.libclang.lib}/lib
            export PATH=$PWD/target/debug:~/.cargo/bin:$PATH
          '';
        nativeBuildInputs = with pkgs; [
          packages.rust-toolchain
          rust-analyzer
        ];
      };
    };
  };
}
