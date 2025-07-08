//! WASI syscalls for lightweight implementation
//!
//! This module implements the essential `wasi_snapshot_preview1` syscalls
//! without the overhead of filesystem, networking, or threading features.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::debug;

use wasmer::{FunctionEnvMut, Memory32, MemorySize, WasmPtr};
use wasmer_wasix_types::wasi::{Clockid, Errno, Filesize, Timestamp};

use crate::{WasiLightEnv, WasiLightError};

// WASI type aliases - using allow to suppress naming convention warnings
#[allow(non_camel_case_types)]
type __wasi_clockid_t = Clockid;
#[allow(non_camel_case_types)]
type __wasi_errno_t = Errno;
#[allow(non_camel_case_types)]
type __wasi_exitcode_t = u32;
#[allow(non_camel_case_types)]
type __wasi_filesize_t = Filesize;
#[allow(non_camel_case_types)]
type __wasi_timestamp_t = Timestamp;

/// Get command line arguments
pub fn args_get<M: MemorySize>(
    ctx: FunctionEnvMut<WasiLightEnv>,
    argv: WasmPtr<WasmPtr<u8, M>, M>,
    argv_buf: WasmPtr<u8, M>,
) -> Result<Errno, WasiLightError> {
    debug!("args_get called");

    let memory_view = ctx
        .data()
        .memory_view(&ctx)
        .ok_or(WasiLightError::MemoryNotAvailable)?;

    let args = &ctx.data().args;
    let mut argv_buf_offset = 0u32;
    let mut argv_offset = 0u32;

    for arg in args {
        let arg_bytes = arg.as_bytes();

        // Write argument string to buffer
        let arg_ptr = argv_buf.add_offset(argv_buf_offset.into())?;
        let arg_slice = arg_ptr.slice(&memory_view, (arg_bytes.len() as u32).into())?;
        arg_slice.write_slice(arg_bytes)?;

        // Write null terminator
        let null_ptr = arg_ptr.add_offset((arg_bytes.len() as u32).into())?;
        null_ptr.write(&memory_view, 0)?;

        // Write pointer to argv array
        let argv_ptr = argv.add_offset(argv_offset.into())?;
        argv_ptr.write(&memory_view, arg_ptr)?;

        argv_buf_offset += arg_bytes.len() as u32 + 1;
        argv_offset += std::mem::size_of::<WasmPtr<u8, M>>() as u32;
    }

    // Write null terminator to argv array
    let null_argv_ptr = argv.add_offset(argv_offset.into())?;
    null_argv_ptr.write(&memory_view, WasmPtr::<u8, M>::null())?;

    Ok(Errno::Success)
}

/// Get argument sizes
pub fn args_sizes_get<M: MemorySize>(
    ctx: FunctionEnvMut<WasiLightEnv>,
    argc: WasmPtr<u32, M>,
    argv_buf_size: WasmPtr<u32, M>,
) -> Result<Errno, WasiLightError> {
    debug!("args_sizes_get called");

    let memory_view = ctx
        .data()
        .memory_view(&ctx)
        .ok_or(WasiLightError::MemoryNotAvailable)?;

    let args = &ctx.data().args;

    // Write argument count
    let argc_ref = argc.deref(&memory_view);
    argc_ref.write(args.len() as u32)?;

    // Calculate total buffer size
    let mut total_size = 0u32;
    for arg in args {
        total_size += arg.len() as u32 + 1; // +1 for null terminator
    }

    // Write buffer size
    let argv_buf_size_ref = argv_buf_size.deref(&memory_view);
    argv_buf_size_ref.write(total_size)?;

    Ok(Errno::Success)
}

/// Get environment variables
pub fn environ_get<M: MemorySize>(
    ctx: FunctionEnvMut<WasiLightEnv>,
    environ: WasmPtr<WasmPtr<u8, M>, M>,
    environ_buf: WasmPtr<u8, M>,
) -> Result<Errno, WasiLightError> {
    debug!("environ_get called");

    let memory_view = ctx
        .data()
        .memory_view(&ctx)
        .ok_or(WasiLightError::MemoryNotAvailable)?;

    let envs = &ctx.data().envs;
    let mut environ_buf_offset = 0u32;
    let mut environ_offset = 0u32;

    for (key, value) in envs {
        let env_var = format!("{key}={value}");
        let env_bytes = env_var.as_bytes();

        // Write environment variable to buffer
        let env_ptr = environ_buf.add_offset(environ_buf_offset.into())?;
        let env_slice = env_ptr.slice(&memory_view, (env_bytes.len() as u32).into())?;
        env_slice.write_slice(env_bytes)?;

        // Write null terminator
        let null_ptr = env_ptr.add_offset((env_bytes.len() as u32).into())?;
        null_ptr.write(&memory_view, 0)?;

        // Write pointer to environ array
        let environ_ptr = environ.add_offset(environ_offset.into())?;
        environ_ptr.write(&memory_view, env_ptr)?;

        environ_buf_offset += env_bytes.len() as u32 + 1;
        environ_offset += std::mem::size_of::<WasmPtr<u8, M>>() as u32;
    }

    // Write null terminator to environ array
    let null_environ_ptr = environ.add_offset(environ_offset.into())?;
    null_environ_ptr.write(&memory_view, WasmPtr::<u8, M>::null())?;

    Ok(Errno::Success)
}

/// Get environment variable sizes
pub fn environ_sizes_get<M: MemorySize>(
    ctx: FunctionEnvMut<WasiLightEnv>,
    environ_count: WasmPtr<u32, M>,
    environ_buf_size: WasmPtr<u32, M>,
) -> Result<Errno, WasiLightError> {
    debug!("environ_sizes_get called");

    let memory_view = ctx
        .data()
        .memory_view(&ctx)
        .ok_or(WasiLightError::MemoryNotAvailable)?;

    let envs = &ctx.data().envs;

    // Write environment variable count
    let environ_count_ref = environ_count.deref(&memory_view);
    environ_count_ref.write(envs.len() as u32)?;

    // Calculate total buffer size
    let mut total_size = 0u32;
    for (key, value) in envs {
        total_size += format!("{key}={value}").as_bytes().len() as u32 + 1; // +1 for null terminator
    }

    // Write buffer size
    let environ_buf_size_ref = environ_buf_size.deref(&memory_view);
    environ_buf_size_ref.write(total_size)?;

    Ok(Errno::Success)
}

/// Get clock resolution
pub fn clock_res_get<M: MemorySize>(
    ctx: FunctionEnvMut<WasiLightEnv>,
    id: __wasi_clockid_t,
    resolution: WasmPtr<__wasi_timestamp_t, M>,
) -> Result<Errno, WasiLightError> {
    debug!("clock_res_get called with id: {:?}", id);

    match id {
        Clockid::Realtime | Clockid::Monotonic => {
            let memory_view = ctx
                .data()
                .memory_view(&ctx)
                .ok_or(WasiLightError::MemoryNotAvailable)?;
            let resolution_ref = resolution.deref(&memory_view);
            // Nanosecond resolution
            resolution_ref.write(1)?;
            Ok(Errno::Success)
        }
        Clockid::ProcessCputimeId | Clockid::ThreadCputimeId => {
            let memory_view = ctx
                .data()
                .memory_view(&ctx)
                .ok_or(WasiLightError::MemoryNotAvailable)?;
            let resolution_ref = resolution.deref(&memory_view);
            // Microsecond resolution
            resolution_ref.write(1000)?;
            Ok(Errno::Success)
        }
        _ => {
            return Ok(Errno::Inval);
        }
    }
}

/// Get clock time
pub fn clock_time_get<M: MemorySize>(
    ctx: FunctionEnvMut<WasiLightEnv>,
    id: __wasi_clockid_t,
    precision: __wasi_timestamp_t,
    time: WasmPtr<__wasi_timestamp_t, M>,
) -> Result<Errno, WasiLightError> {
    debug!(
        "clock_time_get called with id: {:?}, precision: {}",
        id, precision
    );

    let env = ctx.data();

    match id {
        Clockid::Realtime => {
            let memory_view = ctx
                .data()
                .memory_view(&ctx)
                .ok_or(WasiLightError::MemoryNotAvailable)?;
            let time_ref = time.deref(&memory_view);
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO);
            let timestamp = (now + env.clock_offset).as_nanos() as Timestamp;
            time_ref.write(timestamp)?;
            Ok(Errno::Success)
        }
        Clockid::Monotonic => {
            let memory_view = ctx
                .data()
                .memory_view(&ctx)
                .ok_or(WasiLightError::MemoryNotAvailable)?;
            let time_ref = time.deref(&memory_view);
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO);
            let timestamp = (now + env.clock_offset).as_nanos() as Timestamp;
            time_ref.write(timestamp)?;
            Ok(Errno::Success)
        }
        Clockid::ProcessCputimeId | Clockid::ThreadCputimeId => {
            let memory_view = ctx
                .data()
                .memory_view(&ctx)
                .ok_or(WasiLightError::MemoryNotAvailable)?;
            let time_ref = time.deref(&memory_view);
            // For now, return the same as monotonic
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO);
            let timestamp = (now + env.clock_offset).as_nanos() as Timestamp;
            time_ref.write(timestamp)?;
            Ok(Errno::Success)
        }
        _ => {
            return Ok(Errno::Inval);
        }
    }
}

/// Get random bytes
pub fn random_get<M: MemorySize>(
    mut ctx: FunctionEnvMut<WasiLightEnv>,
    buf: WasmPtr<u8, M>,
    buf_len: u32,
) -> Result<Errno, WasiLightError> {
    debug!("random_get called with buf_len: {}", buf_len);

    let random_bytes = ctx.data_mut().random_bytes(buf_len as usize);
    let memory_view = ctx
        .data()
        .memory_view(&ctx)
        .ok_or(WasiLightError::MemoryNotAvailable)?;

    let buf_slice = buf.slice(&memory_view, buf_len.into())?;
    buf_slice.write_slice(&random_bytes)?;

    Ok(Errno::Success)
}

/// Exit process
pub fn proc_exit<M: MemorySize>(
    _ctx: FunctionEnvMut<WasiLightEnv>,
    exit_code: __wasi_exitcode_t,
) -> Result<Errno, WasiLightError> {
    debug!("proc_exit called with exit_code: {}", exit_code);
    Err(WasiLightError::Exit(exit_code))
}

/// Yield to scheduler
pub fn sched_yield<M: MemorySize>(
    _ctx: FunctionEnvMut<WasiLightEnv>,
) -> Result<Errno, WasiLightError> {
    debug!("sched_yield called");
    // In a lightweight implementation, we just return success
    Ok(Errno::Success)
}

/// Raise signal
pub fn proc_raise(_ctx: FunctionEnvMut<WasiLightEnv>, sig: u8) -> Result<Errno, WasiLightError> {
    debug!("proc_raise called with signal: {}", sig);
    // Not supported in lightweight WASI
    Ok(Errno::Notsup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wasmer::{FunctionEnv, Store};

    #[test]
    fn test_clock_res_get() {
        let mut store = Store::default();
        let env = WasiLightEnv::new();
        let func_env = FunctionEnv::new(&mut store, env);

        // Test realtime clock - should fail due to no memory
        let result = clock_res_get::<Memory32>(
            func_env.clone().into_mut(&mut store),
            Clockid::Realtime,
            WasmPtr::new(0),
        );
        assert!(result.is_err());

        // Test invalid clock - should succeed (returns Inval)
        let result = clock_res_get::<Memory32>(
            func_env.into_mut(&mut store),
            Clockid::Unknown, // Invalid clock ID
            WasmPtr::new(0),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Errno::Inval);

        // Test monotonic clock - should fail due to no memory
        let func_env2 = FunctionEnv::new(&mut store, WasiLightEnv::new());
        let result = clock_res_get::<Memory32>(
            func_env2.into_mut(&mut store),
            Clockid::Monotonic,
            WasmPtr::new(0),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_clock_time_get() {
        let mut store = Store::default();
        let env = WasiLightEnv::new().clock_offset(Duration::from_secs(3600));
        let func_env = FunctionEnv::new(&mut store, env);

        // Test realtime clock - should fail due to no memory
        let result = clock_time_get::<Memory32>(
            func_env.clone().into_mut(&mut store),
            Clockid::Realtime,
            0,
            WasmPtr::new(0),
        );
        assert!(result.is_err());

        // Test invalid clock - should succeed (returns Inval)
        let result = clock_time_get::<Memory32>(
            func_env.into_mut(&mut store),
            Clockid::Unknown, // Invalid clock ID
            0,
            WasmPtr::new(0),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Errno::Inval);

        // Test monotonic clock - should fail due to no memory
        let func_env2 = FunctionEnv::new(
            &mut store,
            WasiLightEnv::new().clock_offset(Duration::from_secs(3600)),
        );
        let result = clock_time_get::<Memory32>(
            func_env2.into_mut(&mut store),
            Clockid::Monotonic,
            0,
            WasmPtr::new(0),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_random_get() {
        let mut store = Store::default();
        let env = WasiLightEnv::new().random_seed(42);
        let func_env = FunctionEnv::new(&mut store, env);

        let result =
            random_get::<Memory32>(func_env.clone().into_mut(&mut store), WasmPtr::new(0), 10);
        // This should fail because there's no memory available
        assert!(result.is_err());
    }

    #[test]
    fn test_proc_exit() {
        let mut store = Store::default();
        let env = WasiLightEnv::new();
        let func_env = FunctionEnv::new(&mut store, env);

        let result = proc_exit::<Memory32>(func_env.clone().into_mut(&mut store), 42);
        assert!(result.is_err());

        if let Err(WasiLightError::Exit(code)) = result {
            assert_eq!(code, 42);
        } else {
            panic!("Expected Exit error");
        }
    }

    #[test]
    fn test_proc_raise() {
        let mut store = Store::default();
        let env = WasiLightEnv::new();
        let func_env = FunctionEnv::new(&mut store, env);

        let result = proc_raise(
            func_env.clone().into_mut(&mut store),
            1, // SIGINT
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Errno::Notsup);
    }

    #[test]
    fn test_sched_yield() {
        let mut store = Store::default();
        let env = WasiLightEnv::new();
        let func_env = FunctionEnv::new(&mut store, env);

        let result = sched_yield::<Memory32>(func_env.clone().into_mut(&mut store));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Errno::Success);
    }
}
