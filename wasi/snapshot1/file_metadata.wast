(wasi_test "file_metadata.wasm"
  (preopens ".")
  (assert_return (i64.const 0))
  (assert_stdout "is dir: false\nfiletype: false true false\nfile info: 464\n")
)