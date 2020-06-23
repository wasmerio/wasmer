(wasi_test "fd_sync.wasm"
  (map_dirs ".:test_fs/temp")
  (assert_return (i64.const 0))
  (assert_stdout "170\n1404\n")
)