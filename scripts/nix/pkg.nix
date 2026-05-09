# Nix derivation for the Wasmer CLI binary
#
# NOTE: mostly adapted from upstream nixpkgs
# See: https://github.com/NixOS/nixpkgs/blob/master/pkgs/by-name/wa/wasmer/package.nix
{
  lib,
  stdenv,
  rustPlatform,
  cargo,
  rustc,
  fetchurl,
  llvmPackages_22,
  libffi,
  libxml2,
  withLLVM ? stdenv.hostPlatform.isLinux || (stdenv.hostPlatform.isDarwin && stdenv.hostPlatform.isx86_64),
  withV8 ? (stdenv.hostPlatform.isLinux && stdenv.hostPlatform.isx86_64),
}:

let
  v8Prebuilt = import ./v8-prebuilt.nix { inherit lib stdenv fetchurl; };
in

stdenv.mkDerivation {
  pname = "wasmer";
  version = "dev";

  src = ../..;

  cargoDeps = rustPlatform.importCargoLock {
    lockFile = ../../Cargo.lock;
  };

  nativeBuildInputs = [
    cargo
    rustc
    rustPlatform.cargoSetupHook
    rustPlatform.bindgenHook
  ];

  buildInputs = lib.optionals withLLVM [
    llvmPackages_22.llvm
    libffi
    libxml2
  ];

  preBuild = ''
    if [ ! -f lib/napi/Cargo.toml ]; then
      echo ""
      echo "error: lib/napi submodule is missing (wasmer-napi not found)"
      echo ""
      echo "  When building from a local checkout, Nix does not fetch git submodules"
      echo "  automatically. Re-run with the submodules flag:"
      echo ""
      echo "    nix build '.?submodules=1#wasmer'"
      echo ""
      exit 1
    fi
  '';

  postPatch = lib.optionalString stdenv.hostPlatform.isDarwin ''
    substituteInPlace Makefile \
      --replace-fail 'install: install-wasmer install-capi-headers install-capi-lib install-pkgconfig install-misc' \
                     'install: install-wasmer install-capi-headers install-misc'
  '';

  makeFlags = [
    "WASMER_INSTALL_PREFIX=${placeholder "out"}"
    "DESTDIR=${placeholder "out"}"
    "ENABLE_LLVM=${if withLLVM then "1" else "0"}"
    "ENABLE_NAPI_V8=${if withV8 then "1" else "0"}"
  ];

  buildFlags = [
    "build-wasmer"
    "build-capi"
  ];

  env =
    lib.optionalAttrs withLLVM {
      LLVM_SYS_221_PREFIX = llvmPackages_22.llvm.dev;
    }
    // lib.optionalAttrs withV8 {
      NAPI_V8_INCLUDE_DIR = "${v8Prebuilt}/include";
      V8_LIB_DIR = "${v8Prebuilt}/lib";
    };

  postInstall = lib.optionalString stdenv.hostPlatform.isDarwin ''
    install -Dm755 target/release/libwasmer.dylib $out/lib/libwasmer.dylib
    if pc="$(WASMER_DIR="" target/release/wasmer config --pkg-config 2>/dev/null)"; then
      mkdir -p "$out/lib/pkgconfig"
      printf '%s\n' "$pc" > "$out/lib/pkgconfig/wasmer.pc"
    fi
  '';

  doCheck = false;

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
}
