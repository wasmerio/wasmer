use crate::syscalls::types::{wasi::Timestamp, *};
use tracing::debug;
use wasmer::WasmRef;

pub fn platform_clock_res_get(
    clock_id: wasi_snapshot0::Clockid,
    resolution: WasmRef<Timestamp>,
) -> Result<i64, wasi_snapshot0::Errno> {
    let resolution_val = match clock_id {
        // resolution of monotonic clock at 10ms, from:
        // https://docs.microsoft.com/en-us/windows/desktop/api/sysinfoapi/nf-sysinfoapi-gettickcount64
        wasi_snapshot0::Clockid::Monotonic => 10_000_000,
        // TODO: verify or compute this
        wasi_snapshot0::Clockid::Realtime => 1,
        wasi_snapshot0::Clockid::ProcessCputimeId => {
            return Err(wasi_snapshot0::Errno::Inval);
        }
        wasi_snapshot0::Clockid::ThreadCputimeId => {
            return Err(wasi_snapshot0::Errno::Inval);
        }
        _ => return Err(wasi_snapshot0::Errno::Inval),
    };
    Ok(resolution_val)
}

pub fn platform_clock_time_get(
    clock_id: wasi_snapshot0::Clockid,
    precision: Timestamp,
) -> Result<i64, wasi_snapshot0::Errno> {
    let nanos = match clock_id {
        wasi_snapshot0::Clockid::MONOTONIC => {
            let tick_ms = unsafe { winapi::um::sysinfoapi::GetTickCount64() };
            tick_ms * 1_000_000
        }
        wasi_snapshot0::Clockid::REALTIME => {
            let duration = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| {
                    debug!("Error in wasi::platform_clock_time_get: {:?}", e);
                    wasi_snapshot0::Errno::Io
                })?;
            duration.as_nanos() as u64
        }
        wasi_snapshot0::Clockid::ProcessCputimeId => {
            unimplemented!(
                "wasi::platform_clock_time_get(wasi_snapshot0::Clockid::ProcessCputimeId, ..)"
            )
        }
        wasi_snapshot0::Clockid::ThreadCputimeId => {
            unimplemented!(
                "wasi::platform_clock_time_get(wasi_snapshot0::Clockid::ThreadCputimeId, ..)"
            )
        }
        _ => return Err(wasi_snapshot0::Errno::Inval),
    };
    Ok(nanos as i64)
}
