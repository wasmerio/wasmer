;; Bulk instructions

;; invalid uses

(assert_invalid
  (module
    (type $a (array i8))

    (func (export "array.fill-immutable") (param $1 (ref $a)) (param $2 i32)
      (array.fill $a (local.get $1) (i32.const 0) (local.get $2) (i32.const 0))
    )
  )
  "array is immutable"
)

(assert_invalid
  (module
    (type $a (array (mut i8)))

    (func (export "array.fill-invalid-1") (param $1 (ref $a)) (param $2 funcref)
      (array.fill $a (local.get $1) (i32.const 0) (local.get $2) (i32.const 0))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (type $b (array (mut funcref)))

    (func (export "array.fill-invalid-1") (param $1 (ref $b)) (param $2 i32)
      (array.fill $b (local.get $1) (i32.const 0) (local.get $2) (i32.const 0))
    )
  )
  "type mismatch"
)

(module
  (type $arr8 (array i8))
  (type $arr8_mut (array (mut i8)))

  (global $g_arr8 (ref $arr8) (array.new $arr8 (i32.const 10) (i32.const 12)))
  (global $g_arr8_mut (mut (ref $arr8_mut)) (array.new_default $arr8_mut (i32.const 12)))

  (func (export "array_get_nth") (param $1 i32) (result i32)
    (array.get_u $arr8_mut (global.get $g_arr8_mut) (local.get $1))
  )

  (func (export "array_fill-null")
    (array.fill $arr8_mut (ref.null $arr8_mut) (i32.const 0) (i32.const 0) (i32.const 0))
  )

  (func (export "array_fill") (param $1 i32) (param $2 i32) (param $3 i32)
    (array.fill $arr8_mut (global.get $g_arr8_mut) (local.get $1) (local.get $2) (local.get $3))
  )
)

;; null array argument traps
(assert_trap (invoke "array_fill-null") "null array reference")

;; OOB initial index traps
(assert_trap (invoke "array_fill" (i32.const 13) (i32.const 0) (i32.const 0)) "out of bounds array access")

;; OOB length traps
(assert_trap (invoke "array_fill" (i32.const 0) (i32.const 0) (i32.const 13)) "out of bounds array access")

;; start index = array size, len = 0 doesn't trap
(assert_return (invoke "array_fill" (i32.const 12) (i32.const 0) (i32.const 0)))

;; check arrays were not modified
(assert_return (invoke "array_get_nth" (i32.const 0)) (i32.const 0))
(assert_return (invoke "array_get_nth" (i32.const 5)) (i32.const 0))
(assert_return (invoke "array_get_nth" (i32.const 11)) (i32.const 0))
(assert_trap (invoke "array_get_nth" (i32.const 12)) "out of bounds array access")

;; normal case
(assert_return (invoke "array_fill" (i32.const 2) (i32.const 11) (i32.const 2)))
(assert_return (invoke "array_get_nth" (i32.const 1)) (i32.const 0))
(assert_return (invoke "array_get_nth" (i32.const 2)) (i32.const 11))
(assert_return (invoke "array_get_nth" (i32.const 3)) (i32.const 11))
(assert_return (invoke "array_get_nth" (i32.const 4)) (i32.const 0))
