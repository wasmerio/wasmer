{
  description = "Wasmer Webassembly runtime";

  inputs = {
    # nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flakeutils = {
      url = "github:numtide/flake-utils";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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
            llvmPackages_21.libllvm
            llvmPackages_21.llvm
            llvmPackages_21.llvm.dev
            llvmPackages_21.libclang.dev
            libxml2
            libffi
            cmake
            ninja
            webkitgtk_4_1


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

          shellHook = ''
            export LLVM_SYS_211_PREFIX="${pkgs.llvmPackages_21.llvm.dev}"
            export LIBCLANG_PATH="${pkgs.llvmPackages_21.libclang.lib}/lib"
            export PKG_CONFIG_PATH="${pkgs.webkitgtk_4_1.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
            export LIBRARY_PATH="${pkgs.llvmPackages_21.compiler-rt-libc}/lib/linux:$LIBRARY_PATH"
            export LD_LIBRARY_PATH="${pkgs.llvmPackages_21.compiler-rt-libc}/lib/linux:$LD_LIBRARY_PATH"
            export BINDGEN_EXTRA_CLANG_ARGS="$(
                  < ${pkgs.llvmPackages_21.stdenv.cc}/nix-support/libc-crt1-cflags
                ) $(
                  < ${pkgs.llvmPackages_21.stdenv.cc}/nix-support/libc-cflags
                ) $(
                  < ${pkgs.llvmPackages_21.stdenv.cc}/nix-support/cc-cflags
                ) $(
                  < ${pkgs.llvmPackages_21.stdenv.cc}/nix-support/libcxx-cxxflags
                ) \
                -isystem ${pkgs.glibc.dev}/include \
                -idirafter ${pkgs.llvmPackages_21.clang}/lib/clang/${pkgs.lib.getVersion pkgs.llvmPackages_21.clang}/include"
            '';
        };
      }
    );
}
