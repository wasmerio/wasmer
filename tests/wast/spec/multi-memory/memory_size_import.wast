(module
  (memory (export "mem1") 2 4)
  (memory (export "mem2") 0)
)
(register "M")

(module
  (memory $mem1 (import "M" "mem1") 1 5)
  (memory $mem2 (import "M" "mem2") 0)
  (memory $mem3 3)
  (memory $mem4 4 5)

  (func (export "size1") (result i32) (memory.size $mem1))
  (func (export "size2") (result i32) (memory.size $mem2))
  (func (export "size3") (result i32) (memory.size $mem3))
  (func (export "size4") (result i32) (memory.size $mem4))
)

(assert_return (invoke "size1") (i32.const 2))
(assert_return (invoke "size2") (i32.const 0))
(assert_return (invoke "size3") (i32.const 3))
(assert_return (invoke "size4") (i32.const 4))
