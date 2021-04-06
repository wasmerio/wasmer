(module
  (table $t2 1 externref)
  (table $t3 2 funcref)
  (elem (table $t3) (i32.const 1) func $dummy)
  (func $dummy)

  (func (export "get-externref") (param $i i32) (result externref)
    (table.get $t2 (local.get $i))
  )
  (func $f3 (export "get-funcref") (param $i i32) (result funcref)
    (table.get $t3 (local.get $i))
  )

  (func (export "set-externref") (param $i i32) (param $r externref)
    (table.set $t2 (local.get $i) (local.get $r))
  )
  (func (export "set-funcref") (param $i i32) (param $r funcref)
    (table.set $t3 (local.get $i) (local.get $r))
  )
  (func (export "set-funcref-from") (param $i i32) (param $j i32)
    (table.set $t3 (local.get $i) (table.get $t3 (local.get $j)))
  )

  (func (export "is_null-funcref") (param $i i32) (result i32)
    (ref.is_null (call $f3 (local.get $i)))
  )
)

(assert_return (invoke "get-externref" (i32.const 0)) (ref.null extern))
(assert_return (invoke "set-externref" (i32.const 0) (ref.extern 1)))
(assert_return (invoke "get-externref" (i32.const 0)) (ref.extern 1))
(assert_return (invoke "set-externref" (i32.const 0) (ref.null extern)))
(assert_return (invoke "get-externref" (i32.const 0)) (ref.null extern))

(assert_return (invoke "get-funcref" (i32.const 0)) (ref.null func))
(assert_return (invoke "set-funcref-from" (i32.const 0) (i32.const 1)))
(assert_return (invoke "is_null-funcref" (i32.const 0)) (i32.const 0))
(assert_return (invoke "set-funcref" (i32.const 0) (ref.null func)))
(assert_return (invoke "get-funcref" (i32.const 0)) (ref.null func))

(assert_trap (invoke "set-externref" (i32.const 2) (ref.null extern)) "out of bounds table access")
(assert_trap (invoke "set-funcref" (i32.const 3) (ref.null func)) "out of bounds table access")
(assert_trap (invoke "set-externref" (i32.const -1) (ref.null extern)) "out of bounds table access")
(assert_trap (invoke "set-funcref" (i32.const -1) (ref.null func)) "out of bounds table access")

(assert_trap (invoke "set-externref" (i32.const 2) (ref.extern 0)) "out of bounds table access")
(assert_trap (invoke "set-funcref-from" (i32.const 3) (i32.const 1)) "out of bounds table access")
(assert_trap (invoke "set-externref" (i32.const -1) (ref.extern 0)) "out of bounds table access")
(assert_trap (invoke "set-funcref-from" (i32.const -1) (i32.const 1)) "out of bounds table access")


;; Type errors

(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-index-value-empty-vs-i32-externref
      (table.set $t)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-index-empty-vs-i32
      (table.set $t (ref.null extern))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-value-empty-vs-externref
      (table.set $t (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-size-f32-vs-i32
      (table.set $t (f32.const 1) (ref.null extern))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 funcref)
    (func $type-value-externref-vs-funcref (param $r externref)
      (table.set $t (i32.const 1) (local.get $r))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table $t1 1 externref)
    (table $t2 1 funcref)
    (func $type-value-externref-vs-funcref-multi (param $r externref)
      (table.set $t2 (i32.const 0) (local.get $r))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-result-empty-vs-num (result i32)
      (table.set $t (i32.const 0) (ref.null extern))
    )
  )
  "type mismatch"
)
