# Nix derivation for the Wasmer CLI binary
#
# NOTE: mostly copied from upstrean nixpkgs
# See: https://github.com/NixOS/nixpkgs/blob/master/pkgs/development/interpreters/wasmer/default.nix
pkgs@{ stdenv
, lib
, rustPlatform
, fetchFromGitHub
, llvmPackages
, libffi
, libxml2
, withLLVM ? !stdenv.isDarwin
, withSinglepass ? !(stdenv.isDarwin && stdenv.isx86_64)
, ...
}:

let
  # Get the release version from Cargo.toml
  cargoToml = builtins.fromTOML (builtins.readFile ../../lib/cli/Cargo.toml);
  version = cargoToml.package.version;
in
rustPlatform.buildRustPackage rec {
  pname = "wasmer";
  version = "4.2.5";

  src = ../..;

  cargoLock = {
    lockFile = ../../Cargo.lock;
  };

  nativeBuildInputs = [
    rustPlatform.bindgenHook
  ];

  buildInputs = lib.optionals withLLVM [
    llvmPackages.llvm
    libffi
    libxml2
  ] ++ lib.optionals stdenv.isDarwin [
    pkgs.CoreFoundation
    pkgs.SystemConfiguration
    pkgs.Security
  ];

  # check references to `compiler_features` in Makefile on update
  buildFeatures = [
    "cranelift"
    "wasmer-artifact-create"
    "static-artifact-create"
    "wasmer-artifact-load"
    "static-artifact-load"
  ]
  ++ lib.optional withLLVM "llvm"
  ++ lib.optional withSinglepass "singlepass";

  cargoBuildFlags = [ "--manifest-path" "lib/cli/Cargo.toml" "--bin" "wasmer" ];

  env.LLVM_SYS_150_PREFIX = lib.optionalString withLLVM llvmPackages.llvm.dev;

  doCheck = false;

  meta = with lib; {
    description = "The Universal WebAssembly Runtime";
    longDescription = ''
      Wasmer is a standalone WebAssembly runtime for running WebAssembly outside
      of the browser, supporting WASI. Wasmer can be used
      standalone (via the CLI) and embedded in different languages, running in
      x86 and ARM devices.
    '';
    homepage = "https://wasmer.io/";
    license = licenses.mit;
  };
}
