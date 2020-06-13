(module
  (table $t2 1 anyref)
  (table $t3 2 funcref)
  (elem (table $t3) (i32.const 1) func $dummy)
  (func $dummy)

  (func (export "get-anyref") (param $i i32) (result anyref)
    (table.get $t2 (local.get $i))
  )
  (func $f3 (export "get-funcref") (param $i i32) (result funcref)
    (table.get $t3 (local.get $i))
  )

  (func (export "set-anyref") (param $i i32) (param $r anyref)
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

(assert_return (invoke "get-anyref" (i32.const 0)) (ref.null))
(assert_return (invoke "set-anyref" (i32.const 0) (ref.host 1)))
(assert_return (invoke "get-anyref" (i32.const 0)) (ref.host 1))
(assert_return (invoke "set-anyref" (i32.const 0) (ref.null)))
(assert_return (invoke "get-anyref" (i32.const 0)) (ref.null))

(assert_return (invoke "get-funcref" (i32.const 0)) (ref.null))
(assert_return (invoke "set-funcref-from" (i32.const 0) (i32.const 1)))
(assert_return (invoke "is_null-funcref" (i32.const 0)) (i32.const 0))
(assert_return (invoke "set-funcref" (i32.const 0) (ref.null)))
(assert_return (invoke "get-funcref" (i32.const 0)) (ref.null))

(assert_trap (invoke "set-anyref" (i32.const 2) (ref.null)) "out of bounds")
(assert_trap (invoke "set-funcref" (i32.const 3) (ref.null)) "out of bounds")
(assert_trap (invoke "set-anyref" (i32.const -1) (ref.null)) "out of bounds")
(assert_trap (invoke "set-funcref" (i32.const -1) (ref.null)) "out of bounds")

(assert_trap (invoke "set-anyref" (i32.const 2) (ref.host 0)) "out of bounds")
(assert_trap (invoke "set-funcref-from" (i32.const 3) (i32.const 1)) "out of bounds")
(assert_trap (invoke "set-anyref" (i32.const -1) (ref.host 0)) "out of bounds")
(assert_trap (invoke "set-funcref-from" (i32.const -1) (i32.const 1)) "out of bounds")


;; Type errors

(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-index-value-empty-vs-i32-anyref 
      (table.set $t)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-index-empty-vs-i32
      (table.set $t (ref.null))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-value-empty-vs-anyref
      (table.set $t (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-size-f32-vs-i32
      (table.set $t (f32.const 1) (ref.null))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 funcref)
    (func $type-value-anyref-vs-funcref (param $r anyref)
      (table.set $t (i32.const 1) (local.get $r))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table $t1 1 anyref)
    (table $t2 1 funcref)
    (func $type-value-anyref-vs-funcref-multi (param $r anyref)
      (table.set $t2 (i32.const 0) (local.get $r))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-result-empty-vs-num (result i32)
      (table.set $t (i32.const 0) (ref.null))
    )
  )
  "type mismatch"
)
