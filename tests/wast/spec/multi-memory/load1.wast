(module $M
  (memory (export "mem") 2)

  (func (export "read") (param i32) (result i32)
    (i32.load8_u (local.get 0))
  )
)
(register "M")

(module
  (memory $mem1 (import "M" "mem") 2)
  (memory $mem2 3)

  (data (memory $mem1) (i32.const 20) "\01\02\03\04\05")
  (data (memory $mem2) (i32.const 50) "\0A\0B\0C\0D\0E")

  (func (export "read1") (param i32) (result i32)
    (i32.load8_u $mem1 (local.get 0))
  )
  (func (export "read2") (param i32) (result i32)
    (i32.load8_u $mem2 (local.get 0))
  )
)

(assert_return (invoke $M "read" (i32.const 20)) (i32.const 1))
(assert_return (invoke $M "read" (i32.const 21)) (i32.const 2))
(assert_return (invoke $M "read" (i32.const 22)) (i32.const 3))
(assert_return (invoke $M "read" (i32.const 23)) (i32.const 4))
(assert_return (invoke $M "read" (i32.const 24)) (i32.const 5))

(assert_return (invoke "read1" (i32.const 20)) (i32.const 1))
(assert_return (invoke "read1" (i32.const 21)) (i32.const 2))
(assert_return (invoke "read1" (i32.const 22)) (i32.const 3))
(assert_return (invoke "read1" (i32.const 23)) (i32.const 4))
(assert_return (invoke "read1" (i32.const 24)) (i32.const 5))

(assert_return (invoke "read2" (i32.const 50)) (i32.const 10))
(assert_return (invoke "read2" (i32.const 51)) (i32.const 11))
(assert_return (invoke "read2" (i32.const 52)) (i32.const 12))
(assert_return (invoke "read2" (i32.const 53)) (i32.const 13))
(assert_return (invoke "read2" (i32.const 54)) (i32.const 14))
