wasm_test!(test_fd_allocate, "fd-allocate");
wasm_test!(test_fd_append_after_truncate, "fd-append-after-truncate");
wasm_test!(test_fd_append_seek_read, "fd-append-seek-read");
wasm_test!(test_fd_open_readonly, "fd-open-readonly");
wasm_test!(
    test_fd_sparse_write_after_truncate,
    "fd-sparse-write-after-truncate"
);
