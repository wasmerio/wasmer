//! Signal delivery on the return-to-guest path: the frame the guest's trampoline receives and
//! the sigreturn that follows it (see `signal-callback-register` in the ABI).

use wasixv2_abi::GuestLayout;
use wasixv2_syscalls::{Control, SignalFrame, TaskId};
use wasmer::{AsyncFunctionEnvMut, Function, Global, Instance, RuntimeError, Table, Value};

use crate::ExecEnv;

/// libc's `ucontext_t` (riscv32-shaped, 16-aligned) and the offsets the host reads and writes.
const UCONTEXT_SIZE: u32 = 816;
const UC_STACK: usize = 8;
const UC_SIGMASK: usize = 20;
const UC_MCONTEXT: usize = 160;
/// `mcontext` slots: handler, `sa_flags`, `__stack_pointer`, `__tls_base`.
const MC_HANDLER: usize = UC_MCONTEXT;
const MC_FLAGS: usize = UC_MCONTEXT + 4;
const MC_SP: usize = UC_MCONTEXT + 8;
const MC_TLS: usize = UC_MCONTEXT + 12;
const SIGINFO_SIZE: u32 = 128;

/// The guest exports delivery needs. libc-built modules export all of them.
#[derive(Default)]
pub(crate) struct SignalExports {
    stack_pointer: Option<Global>,
    pub(crate) tls_base: Option<Global>,
    table: Option<Table>,
}

impl SignalExports {
    pub(crate) fn new(instance: &Instance) -> Self {
        let exports = &instance.exports;
        Self {
            stack_pointer: exports.get_global("__stack_pointer").ok().cloned(),
            tls_base: exports.get_global("__tls_base").ok().cloned(),
            table: exports.get_table("__indirect_function_table").ok().cloned(),
        }
    }
}

/// Runs one handler: writes the frame below the stack pointer (or at the top of the alternate
/// stack), calls the trampoline, then restores the mask, stack pointer and TLS base it left in
/// the frame. An error from the handler, such as the exception of a `siglongjmp` or an exit,
/// passes through untouched.
pub(crate) async fn deliver<S: Control + 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    syscalls: &S,
    task: TaskId,
    frame: &SignalFrame,
) -> Result<(), RuntimeError> {
    let (trampoline, uc) = match push_frame(env, frame).await {
        Ok(pushed) => pushed,
        Err(err) => {
            syscalls.signal_return(task, None);
            return Err(err);
        }
    };
    let info = uc + UCONTEXT_SIZE;
    let args = [
        Value::I32(frame.signo()),
        Value::I32(info as i32),
        Value::I32(uc as i32),
    ];
    if let Err(err) = crate::asyncify::call(env, None, &trampoline, &args).await {
        syscalls.signal_return(task, None);
        return Err(err);
    }
    let mask = pop_frame(env, uc).await?;
    syscalls.signal_return(task, Some(mask));
    Ok(())
}

async fn push_frame<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    frame: &SignalFrame,
) -> Result<(Function, u32), RuntimeError> {
    let mut guard = env.write().await;
    let (data, store) = guard.data_and_store_mut();
    let exports = &data.signal;
    let (Some(sp_global), Some(table), Some(memory)) =
        (&exports.stack_pointer, &exports.table, &data.memory)
    else {
        return Err(RuntimeError::new(
            "signal delivery needs the `__stack_pointer` and `__indirect_function_table` exports",
        ));
    };
    let sp = global_u32(sp_global, store);
    let tls = exports
        .tls_base
        .as_ref()
        .map_or(0, |global| global_u32(global, store));
    let top = frame.stack_top.unwrap_or(sp);
    let uc = top
        .checked_sub(UCONTEXT_SIZE + SIGINFO_SIZE)
        .ok_or_else(|| RuntimeError::new("no room for a signal frame"))?
        & !15;

    let mut bytes = vec![0u8; (UCONTEXT_SIZE + SIGINFO_SIZE) as usize];
    let (ss_sp, ss_flags, ss_size) = frame.altstack;
    put(&mut bytes, UC_STACK, ss_sp);
    put(&mut bytes, UC_STACK + 4, ss_flags as u32);
    put(&mut bytes, UC_STACK + 8, ss_size);
    bytes[UC_SIGMASK..UC_SIGMASK + 8].copy_from_slice(&frame.mask.to_le_bytes());
    put(&mut bytes, MC_HANDLER, frame.handler);
    put(&mut bytes, MC_FLAGS, frame.flags);
    put(&mut bytes, MC_SP, sp);
    put(&mut bytes, MC_TLS, tls);
    frame.info.write_to(&mut bytes[UCONTEXT_SIZE as usize..]);
    memory
        .view(&*store)
        .write(uc.into(), &bytes)
        .map_err(|_| RuntimeError::new("signal frame outside guest memory"))?;

    let trampoline = match table.get(&mut *store, frame.trampoline) {
        Some(Value::FuncRef(Some(function))) => function,
        _ => return Err(RuntimeError::new("the signal trampoline is not a function")),
    };
    sp_global.set(&mut *store, Value::I32(uc as i32))?;
    Ok((trampoline, uc))
}

/// The sigreturn: reads back the mask, stack pointer and TLS base from the frame at `uc`.
async fn pop_frame<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    uc: u32,
) -> Result<u64, RuntimeError> {
    let mut guard = env.write().await;
    let (data, store) = guard.data_and_store_mut();
    let memory = data
        .memory
        .as_ref()
        .expect("checked when the frame was pushed");
    let mut bytes = [0u8; MC_TLS + 4];
    memory
        .view(&*store)
        .read(uc.into(), &mut bytes)
        .map_err(|_| RuntimeError::new("signal frame outside guest memory"))?;
    let mask = u64::from_le_bytes(bytes[UC_SIGMASK..UC_SIGMASK + 8].try_into().unwrap());
    let get = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if let Some(global) = &data.signal.stack_pointer {
        global.set(&mut *store, Value::I32(get(MC_SP) as i32))?;
    }
    if let Some(global) = &data.signal.tls_base {
        global.set(&mut *store, Value::I32(get(MC_TLS) as i32))?;
    }
    Ok(mask)
}

fn global_u32(global: &Global, store: &mut impl wasmer::AsStoreMut) -> u32 {
    global.get(store).i32().unwrap_or(0) as u32
}

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
