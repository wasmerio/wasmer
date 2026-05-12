#!/usr/bin/env bash
set -euo pipefail

# Must be run from the repo root
NIX_FILE="scripts/nix/v8-prebuilt.nix"

if [ ! -f "$NIX_FILE" ]; then
  echo "error: $NIX_FILE not found, are you in the repository root?"
  exit 1
fi

cur_v8=$(grep -oP 'v8Version = "\K[^"]+(?=";)' "$NIX_FILE")
echo "Current V8 version: $cur_v8"

if [ ! -f lib/napi/build.rs ]; then
  echo "error: lib/napi/build.rs not found, is the submodule checked out?"
  exit 1
fi

new_v8=$(grep -oP 'PREBUILT_V8_VERSION\s*:\s*&str\s*=\s*"\K[^"]+' lib/napi/build.rs)
echo "Required V8 version: $new_v8"

if [ "$new_v8" = "$cur_v8" ]; then
  echo "V8 version unchanged, nothing to do"
  exit 0
fi

echo "V8 bumped $cur_v8 -> $new_v8, fetching hashes..."
sed -i "s|v8Version = \"[^\"]*\";|v8Version = \"$new_v8\";|" "$NIX_FILE"

base="https://github.com/wasmerio/v8-custom-builds/releases/download/$new_v8"
declare -A assets=(
  ["v8-linux-amd64.tar.xz"]="$base/v8-linux-amd64.tar.xz"
  ["v8-linux-musl-amd64.tar.xz"]="$base/v8-linux-musl-amd64.tar.xz"
  ["v8-darwin-arm64.tar.xz"]="$base/v8-darwin-arm64.tar.xz"
)

for asset in "${!assets[@]}"; do
  url="${assets[$asset]}"
  echo "  Hashing $asset..."
  hash=$(nix-prefetch-url --type sha256 "$url" 2>/dev/null \
         | xargs nix hash convert --hash-algo sha256 --to sri)
  echo "  $asset -> $hash"
  sed -i "s|\"$asset\" = \"[^\"]*\"|\"$asset\" = \"$hash\"|" "$NIX_FILE"
done

echo "Done: $cur_v8 -> $new_v8"
