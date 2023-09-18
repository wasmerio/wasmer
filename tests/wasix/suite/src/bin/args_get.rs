use std::ffi::CStr;

wasix_conformance_suite_shared::declare!(|suite| {
    suite
        .register("args_get")
        .args(["first", "second", "third"]);
});

fn main() {
    unsafe {
        let (num_args, buffer_size) = wasix::args_sizes_get().unwrap();

        let mut args = vec![std::ptr::null_mut(); num_args];
        let mut buffer = vec![0; buffer_size];

        wasix::args_get(args.as_mut_ptr(), buffer.as_mut_ptr()).unwrap();

        let args: Vec<_> = args
            .into_iter()
            .map(|arg| CStr::from_ptr(arg.cast()))
            .map(|s| s.to_str().unwrap())
            .collect();

        assert_eq!(args, &["read_args", "first", "second", "third"]);
    }
}
