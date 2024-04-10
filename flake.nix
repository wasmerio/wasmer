{
  description = "Wasmer Webassembly runtime";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-23.11";
    flakeutils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flakeutils }:
    flakeutils.lib.eachDefaultSystem (system:
      let
        NAME = "wasmer";

        pkgs = import nixpkgs {
          inherit system;
        };
      in
      rec {
        packages.${NAME} = import ./scripts/nix/pkg.nix pkgs;
        defaultPackage = pkgs.callPackage packages.${NAME} pkgs;

        # For `nix run`.
        apps.${NAME} = flakeutils.lib.mkApp {
          drv = packages.${NAME};
        };
        defaultApp = apps.${NAME};

        # Development shell.
        # Run "nix develop" to activate.
        devShell = pkgs.mkShell {
          name = NAME;
          src = self;
          packages = with pkgs; [
            pkg-config
            openssl

            # LLVM and related dependencies
            llvmPackages_15.libllvm
            llvmPackages_15.llvm
            libxml2
            libffi

            # Rust tooling

            # Snapshot testing
            # https://github.com/mitsuhiko/insta
            cargo-insta
            # Test runner
            # https://github.com/nextest-rs/nextest
            cargo-nextest
            # Rust dependency vulnerability checker
            # https://github.com/EmbarkStudios/cargo-deny
            cargo-deny

            # Webassembly tooling

            # "Official" WASM CLI tools
            # (wasm2wat, wat2wasm, wasm-objdump, ...)
            # https://github.com/WebAssembly/wabt
            wabt
            # Provides `wasm-opt` (WASM optimizer) and some other tools
            # https://github.com/WebAssembly/binaryen
            binaryen
            # Various WASM debugging and conversion tools
            # (partial overlap with "wabt")
            # https://github.com/bytecodealliance/wasm-tools
            wasm-tools
          ];

          env.LLVM_SYS_150_PREFIX = pkgs.llvmPackages_15.llvm.dev;
          env.LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
            pkgs.stdenv.cc.cc
            pkgs.openssl.out
          ];
        };
      }
    );
}
