(module
  (func $fn (result i32 exnref)
    i32.const 42
    ref.null exn)
  (func (export "main") (result i32)
    (call $fn)
    (drop))
)

(assert_return (invoke "main") (i32.const 42))
