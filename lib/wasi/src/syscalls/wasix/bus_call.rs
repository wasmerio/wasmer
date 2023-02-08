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
    _ctx: FunctionEnvMut<'_, WasiEnv>,
    _bid: Bid,
    _topic_hash: WasmPtr<WasiHash>,
    _format: BusDataFormat,
    _buf: WasmPtr<u8, M>,
    _buf_len: M::Offset,
    _ret_cid: WasmPtr<Cid, M>,
) -> Result<BusErrno, WasiError> {
    Ok(BusErrno::Unsupported)
}
