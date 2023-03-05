use super::*;
use crate::syscalls::*;

/// ### `callback_spawn()`
/// Sets the callback to invoke upon spawning of new threads
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
#[instrument(level = "debug", skip_all, fields(name = field::Empty, funct_is_some = field::Empty), ret ,err)]
pub fn callback_thread<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), MemoryAccessError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let name = unsafe { name.read_utf8_string(&memory, name_len)? };
    Span::current().record("name", name.as_str());

    let funct = env
        .inner()
        .instance
        .exports
        .get_typed_function(&ctx, &name)
        .ok();
    Span::current().record("funct_is_some", funct.is_some());

    ctx.data_mut().inner_mut().thread_spawn = funct;
    Ok(())
}
