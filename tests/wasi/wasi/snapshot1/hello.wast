(wasi_test "hello.wasm"
  (assert_return (i64.const 0))
  (assert_stdout "Hello, world!\n")
)