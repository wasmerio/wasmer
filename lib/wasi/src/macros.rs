macro_rules! wasi_try {
    ($expr:expr) => {{
        let res: Result<_, crate::syscalls::types::__wasi_errno_t> = $expr;
        match res {
            Ok(val) => val,
            Err(err) => return err,
        }
    }};
    ($expr:expr; $e:expr) => {{
        let opt: Option<_> = $expr;
        wasi_try!(opt.ok_or($e))
    }};
}
