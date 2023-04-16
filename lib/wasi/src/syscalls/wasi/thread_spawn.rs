use super::*;
use crate::syscalls::*;

use wasmer::Memory;
use wasmer_wasix_types::wasi::ThreadStart;

/// ### `thread_spawn()`
/// Creates a new thread by spawning that shares the same
/// memory address space, file handles and main event loops.
///
/// ## Parameters
///
/// * `start_ptr` - Pointer to the structure that describes the thread to be launched
///
/// ## Return
///
/// Returns the thread index of the newly created thread
/// (indices always start from the same value as `pid` and increments in steps)
#[instrument(level = "debug", skip_all, ret)]
pub fn thread_spawn_legacy<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    start_ptr: WasmPtr<ThreadStart<M>, M>,
) -> Tid {
    thread_spawn_internal(&ctx, start_ptr)
        .map_err(|err| anyhow::format_err!("failed to spawn thread - {}", err))
        .unwrap()
}
