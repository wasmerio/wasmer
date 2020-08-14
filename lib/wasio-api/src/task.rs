//! Borrowed from https://github.com/rustwasm/wasm-bindgen/blob/master/crates/futures/src/task/singlethread.rs .

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

struct Inner {
    future: Pin<Box<dyn Future<Output = ()> + 'static>>,
    waker: Waker,
}

pub struct Task {
    inner: RefCell<Option<Inner>>,
}

impl Task {
    pub fn spawn(future: Pin<Box<dyn Future<Output = ()> + 'static>>) {
        let this = Rc::new(Self {
            inner: RefCell::new(None),
        });

        let waker = unsafe { Waker::from_raw(Task::into_raw_waker(Rc::clone(&this))) };

        *this.inner.borrow_mut() = Some(Inner { future, waker });

        Task::wake_by_ref(&this);
    }

    fn wake_by_ref(this: &Rc<Self>) {
        let mut maybe_inner = this.inner.borrow_mut();
        if let Some(ref mut inner) = *maybe_inner {
            let mut cx = Context::from_waker(&inner.waker);
            let poll = inner.future.as_mut().poll(&mut cx);
            if let Poll::Ready(_) = poll {
                *maybe_inner = None;
            }
        }
    }

    unsafe fn into_raw_waker(this: Rc<Self>) -> RawWaker {
        unsafe fn raw_clone(ptr: *const ()) -> RawWaker {
            let ptr = ManuallyDrop::new(Rc::from_raw(ptr as *const Task));
            Task::into_raw_waker((*ptr).clone())
        }

        unsafe fn raw_wake(ptr: *const ()) {
            let ptr = Rc::from_raw(ptr as *const Task);
            Task::wake_by_ref(&ptr);
        }

        unsafe fn raw_wake_by_ref(ptr: *const ()) {
            let ptr = ManuallyDrop::new(Rc::from_raw(ptr as *const Task));
            Task::wake_by_ref(&ptr);
        }

        unsafe fn raw_drop(ptr: *const ()) {
            drop(Rc::from_raw(ptr as *const Task));
        }

        const VTABLE: RawWakerVTable =
            RawWakerVTable::new(raw_clone, raw_wake, raw_wake_by_ref, raw_drop);

        RawWaker::new(Rc::into_raw(this) as *const (), &VTABLE)
    }
}
