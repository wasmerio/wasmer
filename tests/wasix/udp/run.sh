set -e

$WASMER -q run main.wasm --net --dir . -- addr-reuse
$WASMER -q run main.wasm --net --dir . -- ipv6
$WASMER -q run main.wasm --net --dir . -- autobind-connect
$WASMER -q run main.wasm --net --dir . -- autobind-sendto
