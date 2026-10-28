//! Signal delivery on the return-to-guest path: the frame the guest's trampoline receives and
//! the sigreturn that follows it (see `signal-callback-register` in the ABI).

use wasixv2_abi::GuestLayout;
use wasixv2_syscalls::SignalFrame;
use wasmer::{
    AsStoreRef, AsyncFunctionEnvMut, Function, Global, Instance, RuntimeError, Table, Type, Value,
};

use crate::ExecEnv;

/// libc's `ucontext_t` (riscv32-shaped, 16-aligned) and the offsets the host reads and writes.
const UCONTEXT_SIZE: u32 = 816;
const UC_STACK: usize = 8;
const UC_SIGMASK: usize = 20;
const UC_MCONTEXT: usize = 160;
/// `mcontext` slots: handler, `sa_flags`, `__stack_pointer`, `__tls_base`, restart, and the
/// handler's parameter count.
const MC_HANDLER: usize = UC_MCONTEXT;
const MC_FLAGS: usize = UC_MCONTEXT + 4;
const MC_SP: usize = UC_MCONTEXT + 8;
const MC_TLS: usize = UC_MCONTEXT + 12;
const MC_RESTART: usize = UC_MCONTEXT + 16;
const MC_NPARAMS: usize = UC_MCONTEXT + 20;
const SIGINFO_SIZE: u32 = 128;
/// LLVM's wasm red zone: a function that calls nothing may keep this much below
/// `__stack_pointer` without moving it, so a frame on the interrupted stack goes below it. The
/// alternate stack holds nothing live and needs none; its minimum size, `MINSIGSTKSZ`, covers the
/// frame alone.
const RED_ZONE: u32 = 128;

/// The guest exports delivery needs. libc-built modules export all of them.
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
/// stack), calls the trampoline, then restores the stack pointer and TLS base it left in the
/// frame, and returns the mask for `rt_sigreturn`. `restart` says whether the interrupted syscall
/// runs again once the handler returns; the second value is that, after the handler had its say.
/// An error from the handler, such as the exception of a `siglongjmp` or an exit, passes through
/// untouched.
pub(crate) async fn deliver<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    exports: &SignalExports,
    frame: &SignalFrame,
    restart: bool,
) -> Result<(u64, bool), RuntimeError> {
    let (trampoline, uc) = push_frame(env, exports, frame, restart).await?;
    let info = uc + UCONTEXT_SIZE;
    let args = [
        Value::I32(frame.signo()),
        Value::I32(info as i32),
        Value::I32(uc as i32),
    ];
    crate::asyncify::call(env, None, &trampoline, &args).await?;
    let (mask, restart_slot) = pop_frame(env, exports, uc).await?;
    Ok((mask, restart && restart_slot != 0))
}

async fn push_frame<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    exports: &SignalExports,
    frame: &SignalFrame,
    restart: bool,
) -> Result<(Function, u32), RuntimeError> {
    let mut guard = env.write().await;
    let (data, store) = guard.data_and_store_mut();
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
    let top = match frame.altstack_top(sp) {
        Some(top) => Some(top),
        None => sp.checked_sub(RED_ZONE),
    };
    let uc = top
        .and_then(|top| top.checked_sub(UCONTEXT_SIZE + SIGINFO_SIZE))
        .ok_or_else(|| RuntimeError::new("no room for a signal frame"))?
        & !15;
    let nparams = handler_params(table, store, frame.handler)?;

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
    put(&mut bytes, MC_RESTART, restart as u32);
    put(&mut bytes, MC_NPARAMS, nparams);
    frame.info.write_to(&mut bytes[UCONTEXT_SIZE as usize..]);
    crate::memory::write(memory, data.low_guard, &*store, uc, &bytes)
        .map_err(|_| RuntimeError::new("signal frame outside guest memory"))?;

    let trampoline = match table.get(&mut *store, frame.trampoline) {
        Some(Value::FuncRef(Some(function))) => function,
        _ => return Err(RuntimeError::new("the signal trampoline is not a function")),
    };
    sp_global.set(&mut *store, Value::I32(uc as i32))?;
    Ok((trampoline, uc))
}

/// How many parameters the handler at table index `handler` takes, for the trampoline to pass:
/// its type must be `(i32 × n) -> ()` with n at most 3, as `signo`, `info` and `uctx`.
fn handler_params(
    table: &Table,
    store: &mut impl wasmer::AsStoreMut,
    handler: u32,
) -> Result<u32, RuntimeError> {
    let Some(Value::FuncRef(Some(function))) = table.get(&mut *store, handler) else {
        return Err(RuntimeError::new(format!(
            "signal handler {handler} is not a function"
        )));
    };
    let ty = function.ty(&store.as_store_ref());
    let params = ty.params();
    match ty.results().is_empty() && params.len() <= 3 && params.iter().all(|p| *p == Type::I32) {
        true => Ok(params.len() as u32),
        false => Err(RuntimeError::new(format!(
            "signal handler {handler} has type {ty}, which cannot take a signal"
        ))),
    }
}

/// The sigreturn: reads back the mask, stack pointer and TLS base from the frame at `uc`, and the
/// restart slot.
async fn pop_frame<S: 'static>(
    env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    exports: &SignalExports,
    uc: u32,
) -> Result<(u64, u32), RuntimeError> {
    let mut guard = env.write().await;
    let (data, store) = guard.data_and_store_mut();
    let mut bytes = [0u8; MC_RESTART + 4];
    data.read_memory(&*store, uc, &mut bytes)
        .map_err(|_| RuntimeError::new("signal frame outside guest memory"))?;
    let mask = u64::from_le_bytes(bytes[UC_SIGMASK..UC_SIGMASK + 8].try_into().unwrap());
    let get = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if let Some(global) = &exports.stack_pointer {
        global.set(&mut *store, Value::I32(get(MC_SP) as i32))?;
    }
    if let Some(global) = &exports.tls_base {
        global.set(&mut *store, Value::I32(get(MC_TLS) as i32))?;
    }
    Ok((mask, get(MC_RESTART)))
}

fn global_u32(global: &Global, store: &mut impl wasmer::AsStoreMut) -> u32 {
    global.get(store).i32().unwrap_or(0) as u32
}

fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
