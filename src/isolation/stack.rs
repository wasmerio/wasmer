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
    size: usize,
    thread_context: ThreadContext<T, R>,
}

struct ThreadContext<T, R> {
    target: fn(T) -> R,
    param: Option<T>,
    ret: Option<R>,
    jmp_buffer: [c_int; SETJMP_BUFFER_LEN],
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
    pub unsafe fn new(addr: *mut u8, size: usize, f: fn(T) -> R, param: T) -> StackContext<T, R> {
        extern "C" fn run_context<T, R>(ctx: *mut c_void) -> *mut c_void {
            unsafe {
                let ctx = &mut *(ctx as *mut ThreadContext<T, R>);
                if setjmp(&mut ctx.jmp_buffer) != 0 {
                    ctx.ret = Some((ctx.target)(ctx.param.take().unwrap()));
                }
            }
            ::std::ptr::null_mut()
        }

        let mut attr = PthreadAttr::new();
        if pthread_attr_setstack(&mut attr.inner, addr as *mut c_void, size as size_t) != 0 {
            panic!(
                "pthread_attr_setstack failed, addr = {:?}, size = {}",
                addr, size
            );
        }
        let mut thread_ctx = ThreadContext {
            target: f,
            param: Some(param),
            ret: None,
            jmp_buffer: ::std::mem::uninitialized(),
        };
        let mut pthread_handle: pthread_t = ::std::mem::uninitialized();
        if pthread_create(
            &mut pthread_handle,
            &mut attr.inner,
            run_context::<T, R>,
            &mut thread_ctx as *mut ThreadContext<T, R> as *mut c_void,
        ) != 0
        {
            panic!("pthread_create failed");
        }
        pthread_join(pthread_handle, ::std::ptr::null_mut());
        StackContext {
            addr: addr,
            size: size,
            thread_context: thread_ctx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_context() {
        fn some_fn(_: ()) {}
        let mut buf: Vec<u8> = Vec::with_capacity(65536);
        unsafe { buf.set_len(65536) };
        let ctx = unsafe {
            StackContext::new(
                ((buf.as_mut_ptr().offset(buf.len() as isize) as usize) & (!4095usize)) as _,
                buf.len(),
                some_fn,
                (),
            )
        };
    }
}
