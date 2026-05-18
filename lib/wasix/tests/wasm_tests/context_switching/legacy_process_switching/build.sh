#!/usr/bin/env bash
##Config: test_legacy_process_switching_basic_switching
##UnixOnly: true
##Args: basic_switching

##Config: test_legacy_process_switching_vfork_after_switching
##UnixOnly: true
##Args: vfork_after_switching

##Config: test_legacy_process_switching_vfork_after_switching2
##UnixOnly: true
##Args: vfork_after_switching2

##Config: test_legacy_process_switching_fork_after_switching
##UnixOnly: true
##Ignored: flaky test (#6538)
##Args: fork_after_switching

##Config: test_legacy_process_switching_fork_and_vfork_only_work_in_main_context
##UnixOnly: true
##Args: fork_and_vfork_only_work_in_main_context

##Config: test_legacy_process_switching_posix_spawning_a_forking_subprocess_from_a_context
##UnixOnly: true
##Args: posix_spawning_a_forking_subprocess_from_a_context

set -euo pipefail

"$CC" main.c -o main
cp main main.wasm
