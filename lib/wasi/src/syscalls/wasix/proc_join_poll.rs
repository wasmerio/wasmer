use std::task::Waker;

use serde::{Deserialize, Serialize};
use wasmer::FromToNativeWasmType;
use wasmer_wasix_types::wasi::{
    JoinFlags, JoinStatus, JoinStatusType, JoinStatusUnion, OptionPid, WakerId,
};

use super::*;
use crate::{state::conv_waker_id, syscalls::*, WasiProcess};

#[derive(Serialize, Deserialize)]
enum JoinStatusResult {
    Nothing,
    ExitNormal(WasiProcessId, ExitCode),
    Err(Errno),
}

/// ### `proc_join_poll()`
/// Polls to join the child process, blocking this one until the other finishes.
///
/// If the process can not join then it will register a waker that will be woken
/// when the process can be joined
///
/// ## Parameters
///
/// * `pid` - Handle of the child process to wait on
/// * `waker` - Waker ID that will be passed back to the program when the waker is triggered
///
#[instrument(level = "trace", skip_all, fields(pid = ctx.data().process.pid().raw()), ret, err)]
pub fn proc_join_poll<M: MemorySize + 'static>(
    ctx: FunctionEnvMut<'_, WasiEnv>,
    pid_ptr: WasmPtr<OptionPid, M>,
    flags: JoinFlags,
    waker: WakerId,
    status_ptr: WasmPtr<JoinStatus, M>,
) -> Result<Errno, WasiError> {
    let waker = conv_waker_id(ctx.data().state(), waker);
    proc_join_internal(ctx, pid_ptr, flags, status_ptr, Some(&waker))
}
