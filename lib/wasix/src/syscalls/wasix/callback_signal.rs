use super::*;
use crate::syscalls::*;

/// ### `callback_signal()`
/// Sets the callback to invoke signals
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
#[instrument(level = "trace", skip_all, fields(name = field::Empty, funct_is_some = field::Empty), ret)]
pub fn callback_signal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let name = match name.read_utf8_string(&memory, name_len) {
        Ok(a) => a,
        Err(err) => {
            warn!(
                "failed to access memory that holds the name of the signal callback: {}",
                err
            );
            return Ok(());
        }
    };
    Span::current().record("name", name.as_str());

    let funct = env
        .inner()
        .main_module_instance_handles()
        .instance
        .exports
        .get_typed_function(&ctx, &name)
        .ok();
    Span::current().record("funct_is_some", funct.is_some());

    {
        let mut env_inner = ctx.data_mut().inner_mut();
        let inner = env_inner.main_module_instance_handles_mut();
        inner.signal = funct;
        inner.signal_set = true;
    }

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(())
}
