use tracing::debug;
use wasmer::WasmRef;

use crate::syscalls::types::wasi::{self, Timestamp};

pub fn platform_clock_res_get(
    clock_id: wasi::Snapshot0Clockid,
    resolution: WasmRef<Timestamp>,
) -> Result<i64, wasi::Errno> {
    let resolution_val = match clock_id {
        // resolution of monotonic clock at 10ms, from:
        // https://docs.microsoft.com/en-us/windows/desktop/api/sysinfoapi/nf-sysinfoapi-gettickcount64
        wasi::Snapshot0Clockid::Monotonic => 10_000_000,
        // TODO: verify or compute this
        wasi::Snapshot0Clockid::Realtime => 1,
        wasi::Snapshot0Clockid::ProcessCputimeId => {
            return Err(wasi::Errno::Inval);
        }
        wasi::Snapshot0Clockid::ThreadCputimeId => {
            return Err(wasi::Errno::Inval);
        }
        _ => return Err(wasi::Errno::Inval),
    };
    Ok(resolution_val)
}

pub fn platform_clock_time_get(
    clock_id: wasi::Snapshot0Clockid,
    precision: Timestamp,
) -> Result<i64, wasi::Errno> {
    let nanos = match clock_id {
        wasi::Snapshot0Clockid::Monotonic => {
            let tick_ms =
                unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
            tick_ms * 1_000_000
        }
        wasi::Snapshot0Clockid::Realtime => {
            let duration = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| {
                    debug!("Error in wasi::platform_clock_time_get: {:?}", e);
                    wasi::Errno::Io
                })?;
            duration.as_nanos() as u64
        }
        wasi::Snapshot0Clockid::ProcessCputimeId => {
            unimplemented!("wasi::platform_clock_time_get(wasi::Clockid::ProcessCputimeId, ..)")
        }
        wasi::Snapshot0Clockid::ThreadCputimeId => {
            unimplemented!("wasi::platform_clock_time_get(wasi::Clockid::ThreadCputimeId, ..)")
        }
        _ => return Err(wasi::Errno::Inval),
    };
    Ok(nanos as i64)
}
