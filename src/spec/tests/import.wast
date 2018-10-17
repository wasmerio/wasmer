(module
  (func $identity (import "test" "identity") (param i32) (result i32))
  (func (export "exported_func") (param i32) (result i32)
    (call $identity (get_local 0))
  )
)

(assert_return (invoke "exported_func" (i32.const 42)) (i32.const 42))
