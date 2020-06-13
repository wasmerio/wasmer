(module
  (table $t 10 anyref)

  (func (export "fill") (param $i i32) (param $r anyref) (param $n i32)
    (table.fill $t (local.get $i) (local.get $r) (local.get $n))
  )

  (func (export "get") (param $i i32) (result anyref)
    (table.get $t (local.get $i))
  )
)

(assert_return (invoke "get" (i32.const 1)) (ref.null))
(assert_return (invoke "get" (i32.const 2)) (ref.null))
(assert_return (invoke "get" (i32.const 3)) (ref.null))
(assert_return (invoke "get" (i32.const 4)) (ref.null))
(assert_return (invoke "get" (i32.const 5)) (ref.null))

(assert_return (invoke "fill" (i32.const 2) (ref.host 1) (i32.const 3)))
(assert_return (invoke "get" (i32.const 1)) (ref.null))
(assert_return (invoke "get" (i32.const 2)) (ref.host 1))
(assert_return (invoke "get" (i32.const 3)) (ref.host 1))
(assert_return (invoke "get" (i32.const 4)) (ref.host 1))
(assert_return (invoke "get" (i32.const 5)) (ref.null))

(assert_return (invoke "fill" (i32.const 4) (ref.host 2) (i32.const 2)))
(assert_return (invoke "get" (i32.const 3)) (ref.host 1))
(assert_return (invoke "get" (i32.const 4)) (ref.host 2))
(assert_return (invoke "get" (i32.const 5)) (ref.host 2))
(assert_return (invoke "get" (i32.const 6)) (ref.null))

(assert_return (invoke "fill" (i32.const 4) (ref.host 3) (i32.const 0)))
(assert_return (invoke "get" (i32.const 3)) (ref.host 1))
(assert_return (invoke "get" (i32.const 4)) (ref.host 2))
(assert_return (invoke "get" (i32.const 5)) (ref.host 2))

(assert_return (invoke "fill" (i32.const 8) (ref.host 4) (i32.const 2)))
(assert_return (invoke "get" (i32.const 7)) (ref.null))
(assert_return (invoke "get" (i32.const 8)) (ref.host 4))
(assert_return (invoke "get" (i32.const 9)) (ref.host 4))

(assert_return (invoke "fill" (i32.const 9) (ref.null) (i32.const 1)))
(assert_return (invoke "get" (i32.const 8)) (ref.host 4))
(assert_return (invoke "get" (i32.const 9)) (ref.null))

(assert_return (invoke "fill" (i32.const 10) (ref.host 5) (i32.const 0)))
(assert_return (invoke "get" (i32.const 9)) (ref.null))

(assert_trap
  (invoke "fill" (i32.const 8) (ref.host 6) (i32.const 3))
  "out of bounds"
)
(assert_return (invoke "get" (i32.const 7)) (ref.null))
(assert_return (invoke "get" (i32.const 8)) (ref.host 4))
(assert_return (invoke "get" (i32.const 9)) (ref.null))

(assert_trap
  (invoke "fill" (i32.const 11) (ref.null) (i32.const 0))
  "out of bounds"
)

(assert_trap
  (invoke "fill" (i32.const 11) (ref.null) (i32.const 10))
  "out of bounds"
)


;; Type errors

(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-index-value-length-empty-vs-i32-i32
      (table.fill $t)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-index-empty-vs-i32
      (table.fill $t (ref.null) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-value-empty-vs
      (table.fill $t (i32.const 1) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 anyref)
    (func $type-length-empty-vs-i32
      (table.fill $t (i32.const 1) (ref.null))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 0 anyref)
    (func $type-index-f32-vs-i32
      (table.fill $t (f32.const 1) (ref.null) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 0 funcref)
    (func $type-value-vs-funcref (param $r anyref)
      (table.fill $t (i32.const 1) (local.get $r) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 0 anyref)
    (func $type-length-f32-vs-i32
      (table.fill $t (i32.const 1) (ref.null) (f32.const 1))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table $t1 1 anyref)
    (table $t2 1 funcref)
    (func $type-value-anyref-vs-funcref-multi (param $r anyref)
      (table.fill $t2 (i32.const 0) (local.get $r) (i32.const 1))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table $t 1 anyref)
    (func $type-result-empty-vs-num (result i32)
      (table.fill $t (i32.const 0) (ref.null) (i32.const 1))
    )
  )
  "type mismatch"
)
