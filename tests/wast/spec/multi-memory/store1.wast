(module $M1
  (memory (export "mem") 1)

  (func (export "load") (param i32) (result i64)
    (i64.load (local.get 0))
  )
  (func (export "store") (param i32 i64)
    (i64.store (local.get 0) (local.get 1))
  )
)
(register "M1")

(module $M2
  (memory (export "mem") 1)

  (func (export "load") (param i32) (result i64)
    (i64.load (local.get 0))
  )
  (func (export "store") (param i32 i64)
    (i64.store (local.get 0) (local.get 1))
  )
)
(register "M2")

(invoke $M1 "store" (i32.const 0) (i64.const 1))
(invoke $M2 "store" (i32.const 0) (i64.const 2))
(assert_return (invoke $M1 "load" (i32.const 0)) (i64.const 1))
(assert_return (invoke $M2 "load" (i32.const 0)) (i64.const 2))

(module
  (memory $mem1 (import "M1" "mem") 1)
  (memory $mem2 (import "M2" "mem") 1)

  (func (export "load1") (param i32) (result i64)
    (i64.load $mem1 (local.get 0))
  )
  (func (export "load2") (param i32) (result i64)
    (i64.load $mem2 (local.get 0))
  )

  (func (export "store1") (param i32 i64)
    (i64.store $mem1 (local.get 0) (local.get 1))
  )
  (func (export "store2") (param i32 i64)
    (i64.store $mem2 (local.get 0) (local.get 1))
  )
)

(invoke "store1" (i32.const 0) (i64.const 1))
(invoke "store2" (i32.const 0) (i64.const 2))
(assert_return (invoke "load1" (i32.const 0)) (i64.const 1))
(assert_return (invoke "load2" (i32.const 0)) (i64.const 2))
