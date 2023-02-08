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
    _ctx: FunctionEnvMut<'_, WasiEnv>,
    _cid: Cid,
    _format: BusDataFormat,
    _buf: WasmPtr<u8, M>,
    _buf_len: M::Offset,
) -> BusErrno {
    BusErrno::Unsupported
}
