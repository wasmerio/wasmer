#!/usr/bin/env bash
set -euo pipefail

##AbstractConfig: vfork_asyncify
##CurrentDirectory: /home
##MappedDirectory: .:/home
#
##Config: successful_exec:vfork_asyncify
##Args: successful_exec
#
##Config: successful_execlp:vfork_asyncify
##Args: successful_execlp
#
##Config: failing_exec:vfork_asyncify
##Args: failing_exec
#
##Config: cloexec:vfork_asyncify
##Args: cloexec
#
##Config: nested_vfork:vfork_asyncify
##Args: nested_vfork
#
##Config: exiting_child:vfork_asyncify
##Args: exiting_child
#
##Config: trapping_child:vfork_asyncify
##Args: trapping_child

WASIXCC_WASM_EXCEPTIONS=yes WASIXCC_PIC=yes "$CC" main.c -o main -Wl,-pie
cp main main.wasm
