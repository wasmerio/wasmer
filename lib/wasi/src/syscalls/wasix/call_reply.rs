use super::*;
use crate::syscalls::*;

/// Replies to a call that was made to this process
/// from another process; where 'cid' is the call context.
/// This will may also drop the handle and release any
/// associated resources (if keepalive is not set)
///
/// ## Parameters
///
/// * `cid` - Handle of the call to send a reply on
/// * `format` - Format of the data pushed onto the bus
/// * `buf` - The buffer where data to be transmitted is stored
pub fn call_reply<M: MemorySize>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    cid: Cid,
    format: BusDataFormat,
    buf: WasmPtr<u8, M>,
    buf_len: M::Offset,
) -> BusErrno {
    let env = ctx.data();
    let memory = env.memory_view(&ctx);
    let bus = env.runtime.bus();
    trace!(
        "wasi::call_reply (cid={}, format={}, data_len={})",
        cid,
        format,
        buf_len
    );
    let buf_slice = wasi_try_mem_bus!(buf.slice(&memory, buf_len));
    let buf = wasi_try_mem_bus!(buf_slice.read_to_vec());

    let mut guard = env.state.bus.protected();
    if let Some(call) = guard.called.remove(&cid) {
        drop(guard);

        let format = conv_bus_format_from(format);
        call.reply(format, buf);
        BusErrno::Success
    } else {
        BusErrno::Badhandle
    }
}
