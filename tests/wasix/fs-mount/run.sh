#! /bin/sh

set -e

# Direct mount via --volume
$WASMER_RUN main.wasm --volume .:/mount

# Mount via wasmer.toml
$WASMER_RUN .

# Mount via webc package
rm -f fs-mount-test.webc
$WASMER -q package build . -o fs-mount-test.webc
$WASMER_RUN ./fs-mount-test.webc
