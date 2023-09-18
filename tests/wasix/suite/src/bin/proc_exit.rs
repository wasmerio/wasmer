
wasix_conformance_suite_shared::declare!(|suite| {
    suite.register("proc_exit(42)").assert_exit_code(42);
});

fn main() {
    unsafe {
        wasix::proc_exit(42);
    }
}
