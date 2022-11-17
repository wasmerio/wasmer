use super::*;
use crate::syscalls::*;

/// Invokes a call within a running bus process.
///
/// ## Parameters
///
/// * `bid` - Handle of the bus process to invoke the call within
/// * `keep_alive` - Causes the call handle to remain open even when A
///   reply is received. It is then the  callers responsibility
///   to invoke 'bus_drop' when they are finished with the call
/// * `topic` - Topic that describes the type of call to made
/// * `format` - Format of the data pushed onto the bus
/// * `buf` - The buffer where data to be transmitted is stored
pub fn bus_call<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    bid: Bid,
    topic_hash: WasmPtr<WasiHash>,
    format: BusDataFormat,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
    ret_cid: WasmPtr<Cid, M>,
) -> Result<BusErrno, WasiError> {
    let mut env = ctx.data();
    let bus = env.runtime.bus();
    let topic_hash = {
        let memory = env.memory_view(&ctx);
        wasi_try_mem_bus_ok!(topic_hash.read(&memory))
    };
    trace!("wasi::bus_call (bid={}, buf_len={})", bid, buf_len);

    // Get the process that we'll invoke this call for
    let mut guard = env.process.read();
    let bid: WasiProcessId = bid.into();
    let process = if let Some(process) = { guard.bus_processes.get(&bid) } {
        process
    } else {
        return Ok(BusErrno::Badhandle);
    };

    let format = conv_bus_format_from(format);

    // Check if the process has finished
    if let Some(code) = process.inst.exit_code() {
        debug!("process has already exited (code = {})", code);
        return Ok(BusErrno::Aborted);
    }

    // Invoke the call
    let buf = {
        let memory = env.memory_view(&ctx);
        let buf_slice = wasi_try_mem_bus_ok!(buf.slice(&memory, buf_len));
        wasi_try_mem_bus_ok!(buf_slice.read_to_vec())
    };
    let mut invoked = process.inst.invoke(topic_hash, format, buf);
    drop(process);
    drop(guard);

    // Poll the invocation until it does its thing
    let mut invocation;
    {
        invocation = wasi_try_bus_ok!(__asyncify(&mut ctx, None, async move {
            VirtualBusInvokedWait::new(invoked).await.map_err(|err| {
                debug!(
                    "wasi::bus_call failed (bid={}, buf_len={}) - {}",
                    bid, buf_len, err
                );
                Errno::Io
            })
        })
        .map_err(|_| BusErrno::Invoke));
        env = ctx.data();
    }

    // Record the invocation
    let cid = {
        let mut guard = env.state.bus.protected();
        guard.call_seed += 1;
        let cid = guard.call_seed;
        guard.calls.insert(cid, WasiBusCall { bid, invocation });
        cid
    };

    // Now we wake any BUS pollers so that they can drive forward the
    // call to completion - when they poll the call they will also
    // register a BUS waker
    env.state.bus.poll_wake();

    // Return the CID and success to the caller
    let memory = env.memory_view(&ctx);
    wasi_try_mem_bus_ok!(ret_cid.write(&memory, cid));
    Ok(BusErrno::Success)
}