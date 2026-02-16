(module
  (import "spectest" "memory" (memory 1 2))
  (import "spectest" "memory" (memory 1 2))
  (memory $m (import "spectest" "memory") 1 2)
  (import "spectest" "memory" (memory 1 2))
  
  (data (memory 2) (i32.const 10) "\10")

  (func (export "load") (param i32) (result i32) (i32.load $m (local.get 0)))
)

(assert_return (invoke "load" (i32.const 0)) (i32.const 0))
(assert_return (invoke "load" (i32.const 10)) (i32.const 16))
(assert_return (invoke "load" (i32.const 8)) (i32.const 0x100000))
(assert_trap (invoke "load" (i32.const 1000000)) "out of bounds memory access")

