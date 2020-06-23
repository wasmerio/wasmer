(wasi_test "fd_append.wasm"
  (map_dirs ".:test_fs/temp")
  (assert_return (i64.const 0))
  (assert_stdout "\"Hello, world!\\nGoodbye, world!\\n\"\n")
)