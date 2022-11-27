use super::*;
use crate::syscalls::*;

/// Invokes a call within the context of another call
///
/// ## Parameters
///
/// * `parent` - Parent bus call that this is related to
/// * `keep_alive` - Causes the call handle to remain open even when A
///   reply is received. It is then the  callers responsibility
///   to invoke 'bus_drop' when they are finished with the call
/// * `topic` - Topic that describes the type of call to made
/// * `format` - Format of the data pushed onto the bus
/// * `buf` - The buffer where data to be transmitted is stored
pub fn bus_subcall<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    parent_cid: Cid,
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
    trace!(
        "wasi::bus_subcall (parent={}, buf_len={})",
        parent_cid,
        buf_len
    );

    let format = conv_bus_format_from(format);
    let buf = {
        let memory = env.memory_view(&ctx);
        let buf_slice = wasi_try_mem_bus_ok!(buf.slice(&memory, buf_len));
        wasi_try_mem_bus_ok!(buf_slice.read_to_vec())
    };

    // Get the parent call that we'll invoke this call for
    let mut guard = env.state.bus.protected();
    if let Some(parent) = guard.calls.get(&parent_cid) {
        let bid = parent.bid.clone();

        // Invoke the sub-call in the existing parent call
        let mut invoked = parent.invocation.invoke(topic_hash, format, buf);
        drop(parent);
        drop(guard);

        // Poll the invocation until it does its thing
        let invocation;
        {
            invocation = wasi_try_bus_ok!(__asyncify(&mut ctx, None, async move {
                VirtualBusInvokedWait::new(invoked).await.map_err(|err| {
                    debug!(
                        "wasi::bus_subcall failed (parent={}, buf_len={}) - {}",
                        parent_cid, buf_len, err
                    );
                    Errno::Io
                })
            })?
            .map_err(|_| BusErrno::Invoke));
            env = ctx.data();
        }

        // Add the call and return the ID
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
    } else {
        Ok(BusErrno::Badhandle)
    }
}
