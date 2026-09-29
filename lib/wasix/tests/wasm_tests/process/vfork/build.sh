#!/usr/bin/env bash
##AbstractConfig: vfork_asyncify
##CurrentDirectory: /home
##MappedDirectory: .:/home

##Config: successful_exec:vfork_asyncify
##Args: successful_exec

##Config: successful_execlp:vfork_asyncify
##Args: successful_execlp

##Config: failing_exec:vfork_asyncify
##Args: failing_exec

##Config: cloexec:vfork_asyncify
##Args: cloexec

##Config: nested_vfork:vfork_asyncify
##Args: nested_vfork

##Config: exiting_child:vfork_asyncify
##Args: exiting_child

##Config: trapping_child:vfork_asyncify
##Args: trapping_child

##Config: exit_before_exec:vfork_asyncify
##Args: exit_before_exec
##Ignored: undefined behavior in legacy fixture

##Config: trap_before_exec:vfork_asyncify
##Args: trap_before_exec
##Ignored: undefined behavior in legacy fixture

set -euo pipefail

WASIXCC_WASM_EXCEPTIONS=no WASIXCC_PIC=no "$CC" main.c -o main
cp main main.wasm
