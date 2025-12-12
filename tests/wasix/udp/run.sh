set -e

$WASMER_RUN main.wasm --net --dir . -- addr-reuse
$WASMER_RUN main.wasm --net --dir . -- ipv6
$WASMER_RUN main.wasm --net --dir . -- autobind-connect
$WASMER_RUN main.wasm --net --dir . -- autobind-sendto
