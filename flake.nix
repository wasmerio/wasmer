{
  description = "Wasmer Webassembly runtime";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    wasinix = {
      url = "github:wasix-org/wasinix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    self.submodules = true;
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    crane,
    rust-overlay,
    wasinix,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        NAME = "wasmer";

        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        rust-toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        craneLib = (crane.mkLib pkgs).overrideToolchain rust-toolchain;

        withV8 =
          (pkgs.stdenv.hostPlatform.isLinux && pkgs.stdenv.hostPlatform.isx86_64)
          || (pkgs.stdenv.hostPlatform.isDarwin && pkgs.stdenv.hostPlatform.isAarch64);
        withLLVM = pkgs.stdenv.hostPlatform.isLinux || (pkgs.stdenv.hostPlatform.isDarwin && pkgs.stdenv.hostPlatform.isx86_64);

        v8Prebuilt = pkgs.callPackage ./scripts/nix/v8-prebuilt.nix {};
        wasmerPkg = pkgs.callPackage ./scripts/nix/pkg.nix {inherit craneLib v8Prebuilt withV8 withLLVM;};
      in rec {
        packages =
          {${NAME} = wasmerPkg;}
          // pkgs.lib.optionalAttrs withV8 {v8-prebuilt = v8Prebuilt;};
        defaultPackage = packages.${NAME};

        # For `nix run`.
        apps.${NAME} = flake-utils.lib.mkApp {
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
            llvmPackages_22.libllvm
            llvmPackages_22.llvm
            llvmPackages_22.llvm.dev
            llvmPackages_22.libclang.dev
            libxml2
            libffi
            cmake
            ninja
            webkitgtk_4_1

            # Rust tooling
            (rust-toolchain.override {
              targets = ["wasm32-unknown-unknown"];
              extensions = ["clippy" "rustfmt" "rust-analyzer" "rust-src"];
            })

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

            # WASIX C compiler
            wasinix.packages.${system}.wasixcc
          ];

          shellHook = ''
            export LLVM_SYS_221_PREFIX="${pkgs.llvmPackages_22.llvm.dev}"
            export LIBCLANG_PATH="${pkgs.llvmPackages_22.libclang.lib}/lib"
            export PKG_CONFIG_PATH="${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
            export LIBRARY_PATH="${pkgs.llvmPackages_22.compiler-rt-libc}/lib/linux:$LIBRARY_PATH"
            export LD_LIBRARY_PATH="${pkgs.llvmPackages_22.compiler-rt-libc}/lib/linux:$LD_LIBRARY_PATH"
            export NAPI_V8_INCLUDE_DIR="${v8Prebuilt}/include"
            export V8_LIB_DIR="${v8Prebuilt}/lib"
            export BINDGEN_EXTRA_CLANG_ARGS="$(
                  < ${pkgs.llvmPackages_22.stdenv.cc}/nix-support/libc-crt1-cflags
                ) $(
                  < ${pkgs.llvmPackages_22.stdenv.cc}/nix-support/libc-cflags
                ) $(
                  < ${pkgs.llvmPackages_22.stdenv.cc}/nix-support/cc-cflags
                ) $(
                  < ${pkgs.llvmPackages_22.stdenv.cc}/nix-support/libcxx-cxxflags
                ) \
                -isystem ${pkgs.glibc.dev}/include \
                -idirafter ${pkgs.llvmPackages_22.clang}/lib/clang/${pkgs.lib.getVersion pkgs.llvmPackages_22.clang}/include"
          '';
        };
      }
    );
}
