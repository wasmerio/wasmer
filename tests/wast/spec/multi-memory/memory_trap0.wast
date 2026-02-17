(module
    (memory 0)
    (memory 0)
    (memory $m 1)

    (func $addr_limit (result i32)
      (i32.mul (memory.size $m) (i32.const 0x10000))
    )

    (func (export "store") (param $i i32) (param $v i32)
      (i32.store $m (i32.add (call $addr_limit) (local.get $i)) (local.get $v))
    )

    (func (export "load") (param $i i32) (result i32)
      (i32.load $m (i32.add (call $addr_limit) (local.get $i)))
    )

    (func (export "memory.grow") (param i32) (result i32)
      (memory.grow $m (local.get 0))
    )
)

(assert_return (invoke "store" (i32.const -4) (i32.const 42)))
(assert_return (invoke "load" (i32.const -4)) (i32.const 42))
(assert_trap (invoke "store" (i32.const -3) (i32.const 0x12345678)) "out of bounds memory access")
(assert_trap (invoke "load" (i32.const -3)) "out of bounds memory access")
(assert_trap (invoke "store" (i32.const -2) (i32.const 13)) "out of bounds memory access")
(assert_trap (invoke "load" (i32.const -2)) "out of bounds memory access")
(assert_trap (invoke "store" (i32.const -1) (i32.const 13)) "out of bounds memory access")
(assert_trap (invoke "load" (i32.const -1)) "out of bounds memory access")
(assert_trap (invoke "store" (i32.const 0) (i32.const 13)) "out of bounds memory access")
(assert_trap (invoke "load" (i32.const 0)) "out of bounds memory access")
(assert_trap (invoke "store" (i32.const 0x80000000) (i32.const 13)) "out of bounds memory access")
(assert_trap (invoke "load" (i32.const 0x80000000)) "out of bounds memory access")
(assert_return (invoke "memory.grow" (i32.const 0x10001)) (i32.const -1))

