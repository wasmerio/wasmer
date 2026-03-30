set -e

$WASMER_RUN main.wasm --volume . -- basic_switching
$WASMER_RUN main.wasm --volume . -- vfork_after_switching
$WASMER_RUN main.wasm --volume . -- vfork_after_switching2
$WASMER_RUN main.wasm --volume . -- fork_after_switching
$WASMER_RUN main.wasm --volume . -- fork_and_vfork_only_work_in_main_context
$WASMER_RUN main.wasm --volume . -- posix_spawning_a_forking_subprocess_from_a_context
