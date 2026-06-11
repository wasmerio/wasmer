#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONSTANTS_FILE="$REPO_ROOT/.github/ci-constants.env"

pinned="$(grep '^WASIX_LIBC_SYSROOT_TAG=' "$CONSTANTS_FILE" | cut -d= -f2-)"
if [ -z "$pinned" ]; then
  echo "ERROR: WASIX_LIBC_SYSROOT_TAG is not set in $CONSTANTS_FILE"
  exit 1
fi

auth_args=()
if [ -n "${GITHUB_TOKEN:-}" ]; then
  auth_args=(-H "Authorization: Bearer $GITHUB_TOKEN")
fi

latest="$(
  curl -fsSL "${auth_args[@]}" \
    -H "Accept: application/vnd.github+json" \
    "https://api.github.com/repos/wasix-org/wasix-libc/releases/latest" \
    | sed -n 's/.*"tag_name": "\([^"]*\)".*/\1/p' \
    | head -1
)"

if [ -z "$latest" ]; then
  echo "ERROR: failed to fetch latest wasix-libc release tag"
  exit 1
fi

newest="$(printf '%s\n' "$latest" "$pinned" | sort -V | tail -1)"
if [ "$pinned" != "$newest" ]; then
  echo "ERROR: pinned wasix-libc sysroot ($pinned) is older than latest release ($latest)"
  echo "Update WASIX_LIBC_SYSROOT_TAG in .github/ci-constants.env"
  exit 1
fi

echo "Pinned wasix-libc sysroot ($pinned) is equal to or newer than latest release ($latest)"
