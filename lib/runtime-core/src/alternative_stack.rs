mod raw {
    extern "C" {
        pub fn run_on_alternative_stack(
            stack_end: *mut u64,
            stack_begin: *mut u64,
            userdata_arg2: *mut u8,
        ) -> u64;
    }
}

pub(crate) unsafe fn run_on_alternative_stack(stack_end: *mut u64, stack_begin: *mut u64) -> u64 {
    raw::run_on_alternative_stack(stack_end, stack_begin, ::std::ptr::null_mut())
}

pub fn allocate_and_run<R, F: FnOnce() -> R>(size: usize, f: F) -> R {
    struct Context<F: FnOnce() -> R, R> {
        f: Option<F>,
        ret: Option<R>,
    }

    extern "C" fn invoke<F: FnOnce() -> R, R>(_: u64, _: u64, ctx: &mut Context<F, R>) {
        let f = ctx.f.take().unwrap();
        ctx.ret = Some(f());
    }

    unsafe {
        let mut ctx = Context {
            f: Some(f),
            ret: None,
        };
        assert!(size % 16 == 0);
        assert!(size >= 4096);

        let mut stack: Vec<u64> = vec![0; size / 8];
        let mut end_offset = stack.len();

        stack[end_offset - 4] = invoke::<F, R> as usize as u64;
        let stack_begin = stack.as_mut_ptr().offset((end_offset - 4 - 6) as isize);
        let stack_end = stack.as_mut_ptr().offset(end_offset as isize);

        raw::run_on_alternative_stack(
            stack_end,
            stack_begin,
            &mut ctx as *mut Context<F, R> as *mut u8,
        );
        ctx.ret.take().unwrap()
    }
}
