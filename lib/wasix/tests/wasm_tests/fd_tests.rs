wasm_test!(test_fd_allocate, "fd-allocate");
wasm_test!(test_fd_dup2_huge_min, "fd-dup2-huge-min");
wasm_test!(test_fd_append_after_truncate, "fd-append-after-truncate");
wasm_test!(test_fd_append_seek_read, "fd-append-seek-read");
wasm_test!(test_fd_open_readonly, "fd-open-readonly");

wasm_test!(
    test_fd_sparse_write_after_truncate,
    "fd-sparse-write-after-truncate"
);
wasm_test!(
    test_fd_renumber_negative_target,
    "fd-renumber-negative-target"
);
wasm_test!(test_proc_spawn2_dup2_huge_fd, "proc-spawn2-dup2-huge-fd");
wasm_test!(test_proc_spawn2_open_huge_fd, "proc-spawn2-open-huge-fd");

wasm_test!(test_fd_close, "fd-close");

wasm_test!(test_pipes, "pipes");

wasm_test!(test_pwrite_and_size, "pwrite-and-size", temp_dir, stdout = "0");
