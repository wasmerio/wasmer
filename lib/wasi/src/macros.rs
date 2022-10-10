//! Macros to simplify some common WASI-specific tasks.

/// Like the `try!` macro or `?` syntax: returns the value if the computation
/// succeeded or returns the error value.
macro_rules! wasi_try {
    ($expr:expr) => {{
        let res: Result<_, crate::syscalls::types::wasi::Errno> = $expr;
        match res {
            Ok(val) => {
                tracing::trace!("wasi::wasi_try::val: {:?}", val);
                val
            }
            Err(err) => {
                tracing::debug!("wasi::wasi_try::err: {:?}", err);
                return err;
            }
        }
    }};
}

/// Like the `try!` macro or `?` syntax: returns the value if the computation
/// succeeded or returns the error value. Results are wrapped in an Ok
macro_rules! wasi_try_ok {
    ($expr:expr) => {{
        let res: Result<_, crate::syscalls::types::wasi::Errno> = $expr;
        match res {
            Ok(val) => {
                tracing::trace!("wasi::wasi_try_ok::val: {:?}", val);
                val
            }
            Err(err) => {
                tracing::debug!("wasi::wasi_try_ok::err: {:?}", err);
                return Ok(err);
            }
        }
    }};

    ($expr:expr, $thread:expr) => {{
        let res: Result<_, crate::syscalls::types::wasi::Errno> = $expr;
        match res {
            Ok(val) => {
                tracing::trace!("wasi::wasi_try_ok::val: {:?}", val);
                val
            }
            Err(err) => {
                if err == crate::syscalls::types::wasi::Errno::Intr {
                    $thread.yield_now()?;
                }
                tracing::debug!("wasi::wasi_try_ok::err: {:?}", err);
                return Ok(err);
            }
        }
    }};
}

/// Like the `try!` macro or `?` syntax: returns the value if the computation
/// succeeded or returns the error value.
macro_rules! wasi_try_bus {
    ($expr:expr) => {{
        let res: Result<_, crate::syscalls::types::wasi::BusErrno> = $expr;
        match res {
            Ok(val) => {
                tracing::trace!("wasi::wasi_try_bus::val: {:?}", val);
                val
            }
            Err(err) => {
                tracing::debug!("wasi::wasi_try_bus::err: {:?}", err);
                return err;
            }
        }
    }};
}

/// Like `wasi_try` but converts a `MemoryAccessError` to a `wasi::Errno`.
macro_rules! wasi_try_mem {
    ($expr:expr) => {{
        wasi_try!($expr.map_err($crate::mem_error_to_wasi))
    }};
}

/// Like `wasi_try` but converts a `MemoryAccessError` to a `wasi::BusErrno`.
macro_rules! wasi_try_mem_bus {
    ($expr:expr) => {{
        wasi_try_bus!($expr.map_err($crate::mem_error_to_bus))
    }};
}

/// Like `wasi_try` but converts a `MemoryAccessError` to a `wasi::Errno`.
macro_rules! wasi_try_mem_ok {
    ($expr:expr) => {{
        wasi_try_ok!($expr.map_err($crate::mem_error_to_wasi))
    }};

    ($expr:expr, $thread:expr) => {{
        wasi_try_ok!($expr.map_err($crate::mem_error_to_wasi), $thread)
    }};
}

/// Reads a string from Wasm memory.
macro_rules! get_input_str {
    ($memory:expr, $data:expr, $len:expr) => {{
        wasi_try_mem!($data.read_utf8_string($memory, $len))
    }};
}

macro_rules! get_input_str_bus {
    ($memory:expr, $data:expr, $len:expr) => {{
        wasi_try_mem_bus!($data.read_utf8_string($memory, $len))
    }};
}
