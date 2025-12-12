set -e

$WASMER_RUN main.wasm --dir . -- basic_switching
$WASMER_RUN main.wasm --dir . -- vfork_after_switching
$WASMER_RUN main.wasm --dir . -- vfork_after_switching2
$WASMER_RUN main.wasm --dir . -- fork_after_switching
$WASMER_RUN main.wasm --dir . -- fork_and_vfork_only_work_in_main_context
$WASMER_RUN main.wasm --dir . -- posix_spawning_a_forking_subprocess_from_a_context