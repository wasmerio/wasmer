wasm_test!(
    test_libc_clock_function,
    "libc-clock-function",
    stdout = "Clock works."
);
wasm_test!(
    test_libc_getpass,
    "libc-getpass",
    stdout = "getpass test - requires interactive terminal"
);
wasm_test!(
    test_mmap_anon,
    "mmap-anon",
    stdout = "mmap anon memory works"
);
wasm_test!(
    test_variadic_args,
    "variadic-args",
    stdout = "Printing 5, 6, 0, 42"
);

wasm_test!(
    #[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
    test_msync_end_of_file,
    "msync-end-of-file",
    temp_dir,
    stdout = "0"
);

wasm_test!(
    #[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
    test_msync_middle_of_file,
    "msync-middle-of-file",
    temp_dir,
    stdout = "0"
);

wasm_test!(
    #[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
    test_msync_start_of_file,
    "msync-start-of-file",
    temp_dir,
    stdout = "0"
);

wasm_test!(
    #[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
    test_munmap_sync_end_of_file,
    "munmap-sync-end-of-file",
    temp_dir,
    stdout = "0"
);

wasm_test!(
    #[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
    test_munmap_sync_middle_of_file,
    "munmap-sync-middle-of-file",
    temp_dir,
    stdout = "0"
);

wasm_test!(
    #[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
    test_munmap_sync_start_of_file,
    "munmap-sync-start-of-file",
    temp_dir,
    stdout = "0"
);

wasm_test!(
    #[ignore = "file-backed mmap writeback does not currently persist under WasiRunner"]
    test_read_after_munmap,
    "read-after-munmap",
    temp_dir,
    stdout = "0"
);

wasm_test!(test_signal, "signal", stdout = "0");
