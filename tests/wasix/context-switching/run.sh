set -e

$WASMER -q run main.wasm --dir . -- basic_switching
$WASMER -q run main.wasm --dir . -- vfork_after_switching
$WASMER -q run main.wasm --dir . -- vfork_after_switching2
$WASMER -q run main.wasm --dir . -- fork_after_switching
$WASMER -q run main.wasm --dir . -- fork_and_vfork_only_work_in_main_context
$WASMER -q run main.wasm --dir . -- posix_spawning_a_forking_subprocess_from_a_context