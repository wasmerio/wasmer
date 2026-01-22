set -e

$WASMER_RUN main.wasm --net --volume . -- addr-reuse
$WASMER_RUN main.wasm --net --volume . -- ipv6
$WASMER_RUN main.wasm --net --volume . -- autobind-connect
$WASMER_RUN main.wasm --net --volume . -- autobind-sendto
