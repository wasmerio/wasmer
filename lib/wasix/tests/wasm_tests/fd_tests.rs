wasm_test!(test_fd_allocate, "fd-allocate");
wasm_test!(test_fd_dup2_huge_min, "fd-dup2-huge-min");
wasm_test!(test_fd_open_readonly, "fd-open-readonly");
wasm_test!(
    test_fd_renumber_negative_target,
    "fd-renumber-negative-target"
);
wasm_test!(test_proc_spawn2_dup2_huge_fd, "proc-spawn2-dup2-huge-fd");
wasm_test!(test_proc_spawn2_open_huge_fd, "proc-spawn2-open-huge-fd");
