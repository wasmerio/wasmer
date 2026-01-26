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
  (type (func (result exnref exnref v128)))
  (func (type 0) ref.null exn ref.null exn v128.const i32x4 0 0 0 0)
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
