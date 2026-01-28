set -ex

$WASMER_RUN main.wasm "test_unlink"
$WASMER_RUN main.wasm "test_unlink_twice"
$WASMER_RUN main.wasm "test_unlink_twice_with_open_fd"
$WASMER_RUN main.wasm "test_open_after_unlink"
$WASMER_RUN main.wasm "test_new_file_after_unlink_is_new_file"
$WASMER_RUN main.wasm "test_unlink_with_two_fds"
