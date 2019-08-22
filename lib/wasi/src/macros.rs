macro_rules! wasi_try {
    ($expr:expr) => {{
        let res: Result<_, crate::syscalls::types::__wasi_errno_t> = $expr;
        match res {
            Ok(val) => {
                wasmer_runtime_core::trace!("wasi::wasi_try::val: {:?}", val);
                val
            }
            Err(err) => {
                wasmer_runtime_core::trace!("wasi::wasi_try::err: {:?}", err);
                return err;
            }
        }
    }};
    ($expr:expr, $e:expr) => {{
        let opt: Option<_> = $expr;
        wasi_try!(opt.ok_or($e))
    }};
}

macro_rules! get_input_str {
    ($memory:expr, $data:expr, $len:expr) => {{
        wasi_try!($data.get_utf8_string($memory, $len), __WASI_EINVAL)
    }};
}
