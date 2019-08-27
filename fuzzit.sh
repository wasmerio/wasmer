#!/bin/bash
set -xe

# Validate arguments
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <fuzz-type>"
    exit 1
fi

# Configure
NAME=wasmer
TYPE=$1
ROOT=`pwd`

# Setup
if [[ ! -f fuzzit || ! `./fuzzit --version` =~ $FUZZIT_VERSION$ ]]; then
    wget -q -O fuzzit https://github.com/fuzzitdev/fuzzit/releases/latest/download/fuzzit_Linux_x86_64
    chmod a+x fuzzit
fi
./fuzzit --version

# Fuzz
function fuzz {
    FUZZER=$1
    TARGET=$2
    DIR=${3:-.}
    BUILD_DIR=./fuzz/target/x86_64-unknown-linux-gnu/debug
    (
        cd $DIR
        cargo fuzz run $FUZZER -- -runs=0
        $ROOT/fuzzit create job --type $TYPE $NAME/$TARGET $BUILD_DIR/$FUZZER
    )
}
fuzz simple_instantiate simple-instantiate
fuzz validate_wasm validate-wasm
fuzz compile_wasm compile-wasm
