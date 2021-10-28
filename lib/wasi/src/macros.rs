//! Macros to simplify some common WASI-specific tasks.

/// Like the `try!` macro or `?` syntax: returns the value if the computation
/// succeeded or returns the error value.
macro_rules! wasi_try {
    ($expr:expr) => {{
        let res: Result<_, crate::syscalls::types::__wasi_errno_t> = $expr;
        match res {
            Ok(val) => {
                tracing::trace!("wasi::wasi_try::val: {:?}", val);
                val
            }
            Err(err) => {
                tracing::trace!("wasi::wasi_try::err: {:?}", err);
                return err;
            }
        }
    }};
    ($expr:expr, $e:expr) => {{
        let opt: Option<_> = $expr;
        wasi_try!(opt.ok_or($e))
    }};
}

/// Like `wasi_try` but converts a `MemoryAccessError` to a __wasi_errno_t`.
macro_rules! wasi_try_mem {
    ($expr:expr) => {{
        wasi_try!($expr.map_err($crate::mem_error_to_wasi))
    }};
}

/// Reads a string from Wasm memory.
macro_rules! get_input_str {
    ($memory:expr, $data:expr, $len:expr) => {{
        wasi_try_mem!($data.read_utf8_string($memory, $len))
    }};
}
