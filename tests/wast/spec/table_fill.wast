(module
  (table $t 10 externref)

  (func (export "fill") (param $i i32) (param $r externref) (param $n i32)
    (table.fill $t (local.get $i) (local.get $r) (local.get $n))
  )

  (func (export "fill-abbrev") (param $i i32) (param $r externref) (param $n i32)
    (table.fill (local.get $i) (local.get $r) (local.get $n))
  )

  (func (export "get") (param $i i32) (result externref)
    (table.get $t (local.get $i))
  )
)

(assert_return (invoke "get" (i32.const 1)) (ref.null extern))
(assert_return (invoke "get" (i32.const 2)) (ref.null extern))
(assert_return (invoke "get" (i32.const 3)) (ref.null extern))
(assert_return (invoke "get" (i32.const 4)) (ref.null extern))
(assert_return (invoke "get" (i32.const 5)) (ref.null extern))

(assert_return (invoke "fill" (i32.const 2) (ref.extern 1) (i32.const 3)))
(assert_return (invoke "get" (i32.const 1)) (ref.null extern))
(assert_return (invoke "get" (i32.const 2)) (ref.extern 1))
(assert_return (invoke "get" (i32.const 3)) (ref.extern 1))
(assert_return (invoke "get" (i32.const 4)) (ref.extern 1))
(assert_return (invoke "get" (i32.const 5)) (ref.null extern))

(assert_return (invoke "fill" (i32.const 4) (ref.extern 2) (i32.const 2)))
(assert_return (invoke "get" (i32.const 3)) (ref.extern 1))
(assert_return (invoke "get" (i32.const 4)) (ref.extern 2))
(assert_return (invoke "get" (i32.const 5)) (ref.extern 2))
(assert_return (invoke "get" (i32.const 6)) (ref.null extern))

(assert_return (invoke "fill" (i32.const 4) (ref.extern 3) (i32.const 0)))
(assert_return (invoke "get" (i32.const 3)) (ref.extern 1))
(assert_return (invoke "get" (i32.const 4)) (ref.extern 2))
(assert_return (invoke "get" (i32.const 5)) (ref.extern 2))

(assert_return (invoke "fill" (i32.const 8) (ref.extern 4) (i32.const 2)))
(assert_return (invoke "get" (i32.const 7)) (ref.null extern))
(assert_return (invoke "get" (i32.const 8)) (ref.extern 4))
(assert_return (invoke "get" (i32.const 9)) (ref.extern 4))

(assert_return (invoke "fill-abbrev" (i32.const 9) (ref.null extern) (i32.const 1)))
(assert_return (invoke "get" (i32.const 8)) (ref.extern 4))
(assert_return (invoke "get" (i32.const 9)) (ref.null extern))

(assert_return (invoke "fill" (i32.const 10) (ref.extern 5) (i32.const 0)))
(assert_return (invoke "get" (i32.const 9)) (ref.null extern))

(assert_trap
  (invoke "fill" (i32.const 8) (ref.extern 6) (i32.const 3))
  "out of bounds table access"
)
(assert_return (invoke "get" (i32.const 7)) (ref.null extern))
(assert_return (invoke "get" (i32.const 8)) (ref.extern 4))
(assert_return (invoke "get" (i32.const 9)) (ref.null extern))

(assert_trap
  (invoke "fill" (i32.const 11) (ref.null extern) (i32.const 0))
  "out of bounds table access"
)

(assert_trap
  (invoke "fill" (i32.const 11) (ref.null extern) (i32.const 10))
  "out of bounds table access"
)


;; Type errors

(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-index-value-length-empty-vs-i32-i32
      (table.fill $t)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-index-empty-vs-i32
      (table.fill $t (ref.null extern) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-value-empty-vs
      (table.fill $t (i32.const 1) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 10 externref)
    (func $type-length-empty-vs-i32
      (table.fill $t (i32.const 1) (ref.null extern))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 0 externref)
    (func $type-index-f32-vs-i32
      (table.fill $t (f32.const 1) (ref.null extern) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 0 funcref)
    (func $type-value-vs-funcref (param $r externref)
      (table.fill $t (i32.const 1) (local.get $r) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (table $t 0 externref)
    (func $type-length-f32-vs-i32
      (table.fill $t (i32.const 1) (ref.null extern) (f32.const 1))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table $t1 1 externref)
    (table $t2 1 funcref)
    (func $type-value-externref-vs-funcref-multi (param $r externref)
      (table.fill $t2 (i32.const 0) (local.get $r) (i32.const 1))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table $t 1 externref)
    (func $type-result-empty-vs-num (result i32)
      (table.fill $t (i32.const 0) (ref.null extern) (i32.const 1))
    )
  )
  "type mismatch"
)
