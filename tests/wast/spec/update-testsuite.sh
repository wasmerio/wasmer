#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tmp_dir="$(mktemp -d)"

cleanup() {
    rm -rf "${tmp_dir}"
}
trap cleanup EXIT

git clone https://github.com/WebAssembly/spec --depth 1 "${tmp_dir}/spec"
src_dir="${tmp_dir}/spec/test/core"
(
    cd "${src_dir}"
    fd -e wast -t f -0 | xargs -0 -I{} cp -a --parents "{}" "${script_dir}/"
)

# Proposals living in a seperate repositories
mkdir -p "${script_dir}/proposals/wide-arithmetic"
wget -q \
    -O "${script_dir}/proposals/wide-arithmetic/wide-arithmetic.wast" \
    "https://raw.githubusercontent.com/WebAssembly/wide-arithmetic/main/test/core/wide-arithmetic.wast"
