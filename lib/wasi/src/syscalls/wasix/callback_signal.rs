use super::*;
use crate::syscalls::*;

/// ### `callback_signal()`
/// Sets the callback to invoke signals
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
pub fn callback_signal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), WasiError> {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let name = unsafe {
        match name.read_utf8_string(&memory, name_len) {
            Ok(a) => a,
            Err(err) => {
                warn!(
                    "failed to access memory that holds the name of the signal callback: {}",
                    err
                );
                return Ok(());
            }
        }
    };

    let funct = env
        .inner()
        .instance
        .exports
        .get_typed_function(&ctx, &name)
        .ok();
    trace!(
        "wasi[{}:{}]::callback_signal (name={}, found={})",
        ctx.data().pid(),
        ctx.data().tid(),
        name,
        funct.is_some()
    );

    {
        let inner = ctx.data_mut().inner_mut();
        inner.signal = funct;
        inner.signal_set = true;
    }

    let _ = WasiEnv::process_signals_and_exit(&mut ctx)?;

    Ok(())
}
