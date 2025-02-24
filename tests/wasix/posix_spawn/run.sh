set -e

rm -f output.yyy output.zzz

# Run the not-asyncified variant to make sure posix_spawn doesn't require asyncify
$WASMER -q run main-not-asyncified.wasm --dir .
