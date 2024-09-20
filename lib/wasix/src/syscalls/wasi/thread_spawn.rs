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
#[instrument(level = "trace", skip_all, ret)]
pub fn thread_spawn<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    start_ptr: WasmPtr<ThreadStart<M>, M>,
) -> i32 {
    thread_spawn_internal_from_wasi(&mut ctx, start_ptr)
        .map(|tid| tid as i32)
        .map_err(|errno| errno as i32)
        .unwrap_or_else(|err| -err)
}
