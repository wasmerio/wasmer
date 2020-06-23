(wasi_test "path_symlink.wasm"
  (map_dirs "temp:test_fs/temp" "hamlet:test_fs/hamlet")
  (assert_return (i64.const 0))
  (assert_stdout "ACT III\nSCENE I. A room in the castle.\n\n    Enter KING CLAUDIUS,\n")
)