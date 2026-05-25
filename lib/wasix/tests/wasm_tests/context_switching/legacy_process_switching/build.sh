#!/usr/bin/env bash
##AbstractConfig:base
##SkipEngine:V8:async functions are not supported yet
##UnixOnly: true

##BuildEnv: WASIXCC_WASM_EXCEPTIONS=no
##Config: test_legacy_process_switching_basic_switching:base
##Args: basic_switching

##Config: test_legacy_process_switching_vfork_after_switching:base
##Args: vfork_after_switching

##Config: test_legacy_process_switching_vfork_after_switching2:base
##UnixOnly: true
##Args: vfork_after_switching2

##Config: test_legacy_process_switching_fork_after_switching:base
##Ignored: flaky test (#6538)
##Args: fork_after_switching

##Config: test_legacy_process_switching_fork_and_vfork_only_work_in_main_context:base
##Args: fork_and_vfork_only_work_in_main_context

##Config: test_legacy_process_switching_posix_spawning_a_forking_subprocess_from_a_context:base
##Args: posix_spawning_a_forking_subprocess_from_a_context

set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
