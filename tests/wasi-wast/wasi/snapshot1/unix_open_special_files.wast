(wasi_test "unix_open_special_files.wasm"
  (map_dirs "/dev:/dev")
  (assert_return (i64.const 0))
  (assert_stdout "13\n")
)