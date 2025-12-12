set -e

wasixcc -sRUN_WASM_OPT=no main.c -o main-not-asyncified.wasm
wasm-opt --asyncify main-not-asyncified.wasm -o main.wasm

rm -f output.yyy output.zzz

# Run the not-asyncified variant to make sure posix_spawn doesn't require asyncify
$WASMER_RUN main-not-asyncified.wasm --dir .
