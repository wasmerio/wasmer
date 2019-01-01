use ::nix::libc::{
    c_int, c_void, pthread_attr_destroy, pthread_attr_init, pthread_attr_t, pthread_create,
    pthread_join, pthread_t, size_t,
};

const SETJMP_BUFFER_LEN: usize = 27;

extern "C" {
    fn pthread_attr_setstack(
        attr: *mut pthread_attr_t,
        stackaddr: *mut c_void,
        stacksize: size_t,
    ) -> c_int;
    fn setjmp(env: *mut [c_int; SETJMP_BUFFER_LEN]) -> ::nix::libc::c_int;
    fn longjmp(env: *mut [c_int; SETJMP_BUFFER_LEN], val: ::nix::libc::c_int) -> !;
}

pub struct StackContext<T, R> {
    addr: *mut u8,
    thread_context: Box<ThreadContext<T, R>>,
}

pub struct ThreadContext<T, R> {
    target: fn(&mut ThreadContext<T, R>, T) -> R,
    param: Option<T>,
    ret: Option<R>,
    jmp_buffer: [c_int; SETJMP_BUFFER_LEN],
}

impl<T, R> ThreadContext<T, R> {
    /// do_yield swaps the current state with the saved one.
    ///
    /// This is marked as safe because we assume that:
    /// - A ThreadContext can only be constructed from within this module.
    /// - This function is never inlined so that the compiler consider the whole ThreadContext as possibly modified after calling this.
    #[inline(never)] // prevent compiler from looking into do_yield when performing optimizations.
    pub fn do_yield(&mut self) {
        unsafe {
            let mut yield_point = self.jmp_buffer;
            if setjmp(&mut self.jmp_buffer) == 0 {
                longjmp(&mut yield_point, 1);
            }
        }
    }
}

struct PthreadAttr {
    inner: pthread_attr_t,
}

impl PthreadAttr {
    fn new() -> PthreadAttr {
        unsafe {
            let mut inner: pthread_attr_t = ::std::mem::uninitialized();
            if pthread_attr_init(&mut inner) != 0 {
                panic!("pthread_attr_init failed");
            }
            PthreadAttr { inner: inner }
        }
    }
}

impl Drop for PthreadAttr {
    fn drop(&mut self) {
        unsafe {
            if pthread_attr_setstack(&mut self.inner, 65536 as *mut c_void, 65536) != 0 {
                panic!("cannot set stack address to 0");
            }
            pthread_attr_destroy(&mut self.inner);
        }
    }
}

impl<T, R> StackContext<T, R> {
    /// Creates a new stack context.
    ///
    /// This function is unsafe because `addr` and `size` must be ensured by the user to be valid.
    pub unsafe fn new(
        addr: *mut u8,
        f: fn(&mut ThreadContext<T, R>, T) -> R,
        param: T,
    ) -> StackContext<T, R> {
        extern "C" fn run_context<T, R>(ctx: *mut c_void) -> *mut c_void {
            unsafe {
                let ctx = &mut *(ctx as *mut ThreadContext<T, R>);
                if setjmp(&mut ctx.jmp_buffer) != 0 {
                    let target = ctx.target;
                    let param = ctx.param.take().unwrap();
                    let ret = (target)(ctx, param);
                    ctx.ret = Some(ret);
                    ctx.do_yield();
                }
            }
            ::std::ptr::null_mut()
        }

        let mut attr = PthreadAttr::new();

        // The value 65536 is chosen arbitrarily. Any reasonable value for stack size works here.
        if pthread_attr_setstack(&mut attr.inner, addr as *mut c_void, 65536) != 0 {
            panic!("pthread_attr_setstack failed, addr = {:?}", addr);
        }
        let mut thread_ctx = Box::new(ThreadContext {
            target: f,
            param: Some(param),
            ret: None,
            jmp_buffer: ::std::mem::uninitialized(),
        });
        let mut pthread_handle: pthread_t = ::std::mem::uninitialized();
        if pthread_create(
            &mut pthread_handle,
            &mut attr.inner,
            run_context::<T, R>,
            thread_ctx.as_mut() as *mut ThreadContext<T, R> as *mut c_void,
        ) != 0
        {
            panic!("pthread_create failed");
        }
        pthread_join(pthread_handle, ::std::ptr::null_mut());
        StackContext {
            addr: addr,
            thread_context: thread_ctx,
        }
    }

    /// Continues execution of the current stack context.
    ///
    /// This function is unsafe because nested stack context is not supported and may cause UB.
    pub unsafe fn next(mut self) -> Result<R, Self> {
        self.thread_context.do_yield();
        match self.thread_context.ret {
            Some(x) => Ok(x),
            None => Err(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_context() {
        fn some_fn(ctx: &mut ThreadContext<&mut i32, ()>, out: &mut i32) {
            for i in 0..100i32 {
                *out += i;
                ctx.do_yield();
            }
        }
        let mut buf: Vec<u8> = Vec::with_capacity(65536);
        unsafe { buf.set_len(65536) };

        let mut sum: i32 = 0;
        let mut ctx = Some(unsafe {
            StackContext::new(
                ((buf.as_mut_ptr().offset(buf.len() as isize) as usize) & (!4095usize)) as _,
                some_fn,
                &mut sum,
            )
        });
        loop {
            match unsafe { ctx.take().unwrap().next() } {
                Ok(x) => break,
                Err(x) => {
                    ctx = Some(x);
                }
            }
        }
        assert_eq!(sum, 4950);
    }
}
