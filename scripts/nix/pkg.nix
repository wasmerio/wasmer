# Nix derivation for the Wasmer CLI binary
#
# NOTE: mostly adapted from upstream nixpkgs
# See: https://github.com/NixOS/nixpkgs/blob/master/pkgs/by-name/wa/wasmer/package.nix
{
  lib,
  stdenv,
  rustPlatform,
  craneLib,
  v8Prebuilt,
  llvmPackages_22,
  libffi,
  libxml2,
  withLLVM,
  withV8,
}: let
  src = ../..;

  version = (fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;

  cliFeatureList =
    ["cranelift" "wasmer-artifact-create" "static-artifact-create" "wasmer-artifact-load" "static-artifact-load"]
    ++ lib.optional withLLVM "llvm"
    ++ lib.optional withV8 "napi-v8";

  # C API always uses cranelift only
  capiFeatureList = ["wat" "sys-default" "compiler" "wasi" "middlewares" "webc_runner" "cranelift"];

  cliFeatures = lib.concatStringsSep "," cliFeatureList;
  capiFeatures = lib.concatStringsSep "," capiFeatureList;

  commonArgs = {
    inherit src;
    strictDeps = true;

    nativeBuildInputs = [rustPlatform.bindgenHook];

    buildInputs = lib.optionals withLLVM [
      llvmPackages_22.llvm
      libffi
      libxml2
    ];

    env =
      lib.optionalAttrs withLLVM {LLVM_SYS_221_PREFIX = llvmPackages_22.llvm.dev;}
      // lib.optionalAttrs withV8 {
        NAPI_V8_INCLUDE_DIR = "${v8Prebuilt}/include";
        V8_LIB_DIR = "${v8Prebuilt}/lib";
      };

    postPatch = ''
      sed -i '/"tests\/integration\/ios"/d' Cargo.toml
    '';

    preBuild = ''
      if [ ! -f lib/napi/Cargo.toml ]; then
        echo ""
        echo "error: lib/napi submodule is missing (wasmer-napi not found)"
        echo ""
        echo "  Are submodules checked out?"
        echo "  If you're using this repository as a flake input, make sure you set"
        echo "    inputs.wasmer = { url = \"...\"; submodules = true; };"
        echo ""
        echo "  If you're using an old Nix version,"
        echo "  you might need to manually tell nix to checkout submodules via the path:"
        echo ""
        echo "    nix build '.?submodules=1#wasmer'"
        echo ""
        echo "   inputs.wasmer.url = \"git+https://github.com/me/my-lib?submodules=1\";"
        echo ""
        exit 1
      fi
    '';

    doCheck = false;
  };

  # Pre-build all deps in one pass. Changing only Rust source skips this entirely.
  cargoArtifacts = craneLib.buildDepsOnly (commonArgs
    // {
      pname = "wasmer-deps";
      inherit version;
      buildPhaseCargoCommand = ''
        cargo build --release -p wasmer-cli --features ${cliFeatures} --bin wasmer --locked
        cargo build --release -p wasmer-c-api --no-default-features --features ${capiFeatures} --locked
      '';
    });
in
  craneLib.mkCargoDerivation (commonArgs
    // {
      pname = "wasmer";
      inherit version;

      inherit cargoArtifacts;

      buildPhaseCargoCommand = ''
        WASMER_INSTALL_PREFIX=$out \
          cargo build --release \
            -p wasmer-cli \
            --features ${cliFeatures} \
            --bin wasmer \
            --locked

        cargo build --release \
          -p wasmer-c-api \
          --no-default-features \
          --features ${capiFeatures} \
          --locked
      '';

      installPhaseCommand =
        ''
          install -Dm755 target/release/wasmer $out/bin/wasmer

          install -Dm644 lib/c-api/wasmer.h $out/include/wasmer.h
          install -Dm644 lib/c-api/wasmer_wasm.h $out/include/wasmer_wasm.h
          install -Dm644 lib/c-api/tests/wasm-c-api/include/wasm.h $out/include/wasm.h
          install -Dm644 lib/c-api/tests/wasm-c-api/include/wasm.hh $out/include/wasm.hh
          install -Dm644 lib/c-api/README.md $out/include/wasmer-README.md
          install -Dm644 LICENSE $out/share/licenses/wasmer/LICENSE
        ''
        + lib.optionalString stdenv.hostPlatform.isLinux ''
          install -Dm755 target/release/libwasmer.so $out/lib/libwasmer.so
          mkdir -p "$out/lib/pkgconfig"
          env -u WASMER_DIR $out/bin/wasmer config --pkg-config > "$out/lib/pkgconfig/wasmer.pc"
        ''
        + lib.optionalString stdenv.hostPlatform.isDarwin ''
          install -Dm755 target/release/libwasmer.dylib $out/lib/libwasmer.dylib
          mkdir -p "$out/lib/pkgconfig"
          env -u WASMER_DIR $out/bin/wasmer config --pkg-config > "$out/lib/pkgconfig/wasmer.pc"
        '';

      meta = {
        description = "Universal WebAssembly Runtime";
        mainProgram = "wasmer";
        longDescription = ''
          Wasmer is a standalone WebAssembly runtime for running WebAssembly outside
          of the browser, supporting WASI and Emscripten. Wasmer can be used
          standalone (via the CLI) and embedded in different languages, running in
          x86 and ARM devices.
        '';
        homepage = "https://wasmer.io/";
        license = lib.licenses.mit;
      };
    })
