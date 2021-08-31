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

/// Reads a string from Wasm memory and returns the invalid argument error
/// code if it fails.
///
/// # Safety
/// See the safety docs for [`wasmer::WasmPtr::get_utf8_str`]: the returned value
/// points into Wasm memory and care must be taken that it does not get
/// corrupted.
macro_rules! get_input_str {
    ($memory:expr, $data:expr, $len:expr) => {{
        wasi_try!($data.get_utf8_string($memory, $len), __WASI_EINVAL)
    }};
}
