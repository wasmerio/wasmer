use super::*;
use crate::syscalls::*;

/// Spawns a new bus process for a particular web WebAssembly
/// binary that is referenced by its process name on a remote instance.
///
/// ## Parameters
///
/// * `name` - Name of the process to be spawned
/// * `reuse` - Indicates if the existing processes should be reused
///   if they are already running
/// * `instance` - Instance identifier where this process will be spawned
/// * `token` - Acceess token used to authenticate with the instance
///
/// ## Return
///
/// Returns a bus process id that can be used to invoke calls
pub fn bus_open_remote<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
    reuse: Bool,
    instance: WasmPtr<u8, M>,
    instance_len: M::Offset,
    token: WasmPtr<u8, M>,
    token_len: M::Offset,
    ret_bid: WasmPtr<Bid, M>,
) -> Result<BusErrno, WasiError> {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name = unsafe { get_input_str_bus_ok!(&memory, name, name_len) };
    let instance = unsafe { get_input_str_bus_ok!(&memory, instance, instance_len) };
    let token = unsafe { get_input_str_bus_ok!(&memory, token, token_len) };
    let reuse = reuse == Bool::True;
    debug!(
        "wasi::bus_open_remote (name={}, reuse={}, instance={})",
        name, reuse, instance
    );

    bus_open_internal(ctx, name, reuse, Some(instance), Some(token), ret_bid)
}

pub(crate) fn bus_open_internal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: String,
    reuse: bool,
    instance: Option<String>,
    token: Option<String>,
    ret_bid: WasmPtr<Bid, M>,
) -> Result<BusErrno, WasiError> {
    let env = ctx.data();
    let bus = env.runtime.bus();
    let memory = env.memory_view(&ctx);
    let name: Cow<'static, str> = name.into();

    // Check if it already exists
    if reuse {
        let guard = env.process.read();
        if let Some(bid) = guard.bus_process_reuse.get(&name) {
            if guard.bus_processes.contains_key(bid) {
                wasi_try_mem_bus_ok!(ret_bid.write(&memory, bid.clone().into()));
                return Ok(BusErrno::Success);
            }
        }
    }

    let (handles, ctx) = wasi_try_bus_ok!(proc_spawn_internal(
        ctx,
        name.to_string(),
        None,
        None,
        None,
        WasiStdioMode::Null,
        WasiStdioMode::Null,
        WasiStdioMode::Log
    ));
    let env = ctx.data();
    let memory = env.memory_view(&ctx);

    let pid: WasiProcessId = handles.bid.into();
    let memory = env.memory_view(&ctx);
    {
        let mut inner = env.process.write();
        inner.bus_process_reuse.insert(name, pid);
    };

    wasi_try_mem_bus_ok!(ret_bid.write(&memory, pid.into()));
    Ok(BusErrno::Success)
}