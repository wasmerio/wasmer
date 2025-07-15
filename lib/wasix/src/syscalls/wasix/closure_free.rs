use crate::syscalls::*;

/// Free a previously allocated slot for a closure in the `__indirect_function_table`
///
/// After calling this it is undefined behavior to call the function at the given index.
#[instrument(level = "trace", fields(%closure), ret)]
pub fn closure_free<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    closure: u32,
) -> Result<Errno, WasiError> {
    WasiEnv::do_pending_operations(&mut ctx)?;

    let (env, mut store) = ctx.data_and_store_mut();

    let Some(linker) = env.inner().linker().cloned() else {
        error!("Closures only work for dynamic modules.");
        return Ok(Errno::Notsup);
    };

    let free_result = linker.free_closure_index(&mut ctx, closure);
    if let Err(e) = free_result {
        // Should never happen
        panic!("Failed to free closure index: {e}");
    }

    return Ok(Errno::Success);
}
