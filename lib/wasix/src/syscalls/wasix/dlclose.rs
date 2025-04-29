use super::*;
use crate::syscalls::*;

#[instrument(level = "trace", skip_all, fields(path = field::Empty), ret)]
pub fn dlclose<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    handle: DlHandle,
    err_buf: WasmPtr<u8, M>,
    err_buf_len: M::Offset,
) -> Result<Errno, WasiError> {
    // TODO: call dtors, preferably in the linker!
    todo!();
}
