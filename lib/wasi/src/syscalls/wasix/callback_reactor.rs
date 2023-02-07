use super::*;
use crate::syscalls::*;

/// ### `callback_reactor()`
/// Sets the callback to invoke for reactors
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
pub fn callback_reactor<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), MemoryAccessError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let name = unsafe { name.read_utf8_string(&memory, name_len)? };
    debug!(
        "wasi[{}:{}]::callback_reactor (name={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name
    );

    let funct = env
        .inner()
        .instance
        .exports
        .get_typed_function(&ctx, &name)
        .ok();

    ctx.data_mut().inner_mut().react = funct;
    Ok(())
}
