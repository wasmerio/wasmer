{
  lib,
  stdenv,
  fetchurl,
  writeShellApplication,
  nix,
  gnugrep,
  gnused,
  findutils,
}: let
  v8Version = "11.9.2";

  v8Hashes = {
    "v8-linux-amd64.tar.xz" = "sha256-nTCVdBKtyVMb7lE+Db4RDsShKkLbG/0r980ejd+EAvo=";
    "v8-linux-musl-amd64.tar.xz" = "sha256-XgRs3I46B2PG7Jrv5E+KSeuNfXLhgB7R66cAkA/Bvv8=";
    "v8-darwin-arm64.tar.xz" = "sha256-xAG1PcAGw8a0A9k8d78/whTUXnqdfRZBz8yrg/+iz0M=";
  };

  assetName =
    if stdenv.hostPlatform.isLinux && stdenv.hostPlatform.isx86_64 && stdenv.hostPlatform.isMusl
    then "v8-linux-musl-amd64.tar.xz"
    else if stdenv.hostPlatform.isLinux && stdenv.hostPlatform.isx86_64
    then "v8-linux-amd64.tar.xz"
    else if stdenv.hostPlatform.isDarwin && stdenv.hostPlatform.isAarch64
    then "v8-darwin-arm64.tar.xz"
    else throw "V8 prebuilt is not available for ${stdenv.hostPlatform.system}";
in
  stdenv.mkDerivation {
    name = "wasmer-v8-prebuilt-${v8Version}";
    src = fetchurl {
      url = "https://github.com/wasmerio/v8-custom-builds/releases/download/${v8Version}/${assetName}";
      hash = v8Hashes.${assetName};
    };
    sourceRoot = ".";
    dontBuild = true;
    installPhase = ''
      cp -r . $out
    '';
    passthru.updateScript = writeShellApplication {
      name = "update-wasmer-v8";
      runtimeInputs = [nix gnugrep gnused findutils];
      text = builtins.readFile ./update-v8.sh;
    };

    meta.sourceProvenance = [lib.sourceTypes.binaryNativeCode];
  }
