(module
  (table $t0 0 anyref)
  (table $t1 1 anyref)
  (table $t2 0 2 anyref)
  (table $t3 3 8 anyref)

  (func (export "size-t0") (result i32) (table.size $t0))
  (func (export "size-t1") (result i32) (table.size $t1))
  (func (export "size-t2") (result i32) (table.size $t2))
  (func (export "size-t3") (result i32) (table.size $t3))

  (func (export "grow-t0") (param $sz i32)
    (drop (table.grow $t0 (ref.null) (local.get $sz)))
  )
  (func (export "grow-t1") (param $sz i32)
    (drop (table.grow $t1 (ref.null) (local.get $sz)))
  )
  (func (export "grow-t2") (param $sz i32)
    (drop (table.grow $t2 (ref.null) (local.get $sz)))
  )
  (func (export "grow-t3") (param $sz i32)
    (drop (table.grow $t3 (ref.null) (local.get $sz)))
  )
)

(assert_return (invoke "size-t0") (i32.const 0))
(assert_return (invoke "grow-t0" (i32.const 1)))
(assert_return (invoke "size-t0") (i32.const 1))
(assert_return (invoke "grow-t0" (i32.const 4)))
(assert_return (invoke "size-t0") (i32.const 5))
(assert_return (invoke "grow-t0" (i32.const 0)))
(assert_return (invoke "size-t0") (i32.const 5))

(assert_return (invoke "size-t1") (i32.const 1))
(assert_return (invoke "grow-t1" (i32.const 1)))
(assert_return (invoke "size-t1") (i32.const 2))
(assert_return (invoke "grow-t1" (i32.const 4)))
(assert_return (invoke "size-t1") (i32.const 6))
(assert_return (invoke "grow-t1" (i32.const 0)))
(assert_return (invoke "size-t1") (i32.const 6))

(assert_return (invoke "size-t2") (i32.const 0))
(assert_return (invoke "grow-t2" (i32.const 3)))
(assert_return (invoke "size-t2") (i32.const 0))
(assert_return (invoke "grow-t2" (i32.const 1)))
(assert_return (invoke "size-t2") (i32.const 1))
(assert_return (invoke "grow-t2" (i32.const 0)))
(assert_return (invoke "size-t2") (i32.const 1))
(assert_return (invoke "grow-t2" (i32.const 4)))
(assert_return (invoke "size-t2") (i32.const 1))
(assert_return (invoke "grow-t2" (i32.const 1)))
(assert_return (invoke "size-t2") (i32.const 2))

(assert_return (invoke "size-t3") (i32.const 3))
(assert_return (invoke "grow-t3" (i32.const 1)))
(assert_return (invoke "size-t3") (i32.const 4))
(assert_return (invoke "grow-t3" (i32.const 3)))
(assert_return (invoke "size-t3") (i32.const 7))
(assert_return (invoke "grow-t3" (i32.const 0)))
(assert_return (invoke "size-t3") (i32.const 7))
(assert_return (invoke "grow-t3" (i32.const 2)))
(assert_return (invoke "size-t3") (i32.const 7))
(assert_return (invoke "grow-t3" (i32.const 1)))
(assert_return (invoke "size-t3") (i32.const 8))


;; Type errors

(assert_invalid
  (module
    (table $t 1 anyref)
    (func $type-result-i32-vs-empty
      (table.size $t)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 1 anyref)
    (func $type-result-i32-vs-f32 (result f32)
      (table.size $t)
    )
  )
  "type mismatch"
)
