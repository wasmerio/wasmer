(module
  (func $fn (result i32 exnref)
    i32.const 42
    ref.null exn)
  (func (export "main") (result i32)
    (call $fn)
    (drop))
)
(assert_return (invoke "main") (i32.const 42))

(module
  (func $fn (result f64 exnref i32) f64.const 0 ref.null exn i32.const 0)
)

(module
  (func (export "return_42") (result i64)
    (block
        br 0
        (try_table)
    )
    i64.const 42)
)
(assert_return (invoke "return_42") (i64.const 42))

(module
  (func (result funcref exnref i32)
    ref.null func
    ref.null exn
    i32.const 0)
)

(module
(func (export "exnref_is_null") (result i32)
    (local $e exnref)
    local.get $e
    ref.is_null
  )
)

(assert_return (invoke "exnref_is_null") (i32.const 1))
