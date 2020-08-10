#[macro_export]
macro_rules! c_try {
    ($expr:expr) => {{
        let res: Result<_, _> = $expr;
        match res {
            Ok(val) => val,
            Err(err) => {
                crate::error::update_last_error(err);
                return None;
            }
        }
    }};
    ($expr:expr, $e:expr) => {{
        let opt: Option<_> = $expr;
        c_try!(opt.ok_or_else(|| $e))
    }};
}
