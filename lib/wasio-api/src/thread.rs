use crate::types::*;
use crate::{executor::IoFuture, sys};
use futures::Future;
use std::convert::TryFrom;
use std::mem::MaybeUninit;
use std::time::Duration;

pub fn delay(dur: Duration) -> impl Future<Output = __wasi_errno_t> {
    let nanos = u64::try_from(dur.as_nanos()).unwrap();
    IoFuture::new(move |uctx| unsafe {
        let mut ct = CancellationToken(0);
        let e = sys::delay(nanos, uctx, &mut ct);
        if e != 0 {
            Err(e)
        } else {
            Ok(ct)
        }
    })
}

pub fn async_nop() -> impl Future<Output = __wasi_errno_t> {
    IoFuture::new(move |uctx| unsafe {
        let mut ct = CancellationToken(0);
        let e = sys::async_nop(uctx, &mut ct);
        if e != 0 {
            Err(e)
        } else {
            Ok(ct)
        }
    })
}
