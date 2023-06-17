use super::*;
use crate::syscalls::*;

/// ### `callback_waker_wake()`
///
/// Sets the callback that the runtime will execute whenever a task waker
/// has been triggered and needs to be woken up
///
/// The default callback that will be invoked is `_waker_wake`
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
#[instrument(level = "trace", skip_all, fields(name = field::Empty, funct_is_some = field::Empty), ret, err)]
pub fn callback_waker_wake<M: MemorySize>(
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

    let funct = unsafe { env.inner() }
        .instance
        .exports
        .get_typed_function(&ctx, &name)
        .ok();
    Span::current().record("funct_is_some", funct.is_some());

    {
        let mut inner = ctx.data_mut().try_inner_mut().unwrap();
        inner.waker_wake = funct;
    }

    let _ = unsafe { WasiEnv::process_signals_and_wakes_and_exit(&mut ctx)? };

    Ok(())
}
