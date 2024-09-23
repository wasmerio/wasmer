;; Bulk instructions

;; invalid uses

(assert_invalid
  (module
    (type $a (array i8))

    (data $d1 "a")

    (func (export "array.init_data-immutable") (param $1 (ref $a))
      (array.init_data $a $d1 (local.get $1) (i32.const 0) (i32.const 0) (i32.const 0))
    )
  )
  "array is immutable"
)

(assert_invalid
  (module
    (type $a (array (mut funcref)))

    (data $d1 "a")

    (func (export "array.init_data-invalid-1") (param $1 (ref $a))
      (array.init_data $a $d1 (local.get $1) (i32.const 0) (i32.const 0) (i32.const 0))
    )
  )
  "array type is not numeric or vector"
)

(module
  (type $arr8 (array i8))
  (type $arr8_mut (array (mut i8)))
  (type $arr16_mut (array (mut i16)))

  (global $g_arr8 (ref $arr8) (array.new $arr8 (i32.const 10) (i32.const 12)))
  (global $g_arr8_mut (mut (ref $arr8_mut)) (array.new_default $arr8_mut (i32.const 12)))
  (global $g_arr16_mut (ref $arr16_mut) (array.new_default $arr16_mut (i32.const 6)))

  (data $d1 "abcdefghijkl")

  (func (export "array_get_nth") (param $1 i32) (result i32)
    (array.get_u $arr8_mut (global.get $g_arr8_mut) (local.get $1))
  )

  (func (export "array_get_nth_i16") (param $1 i32) (result i32)
    (array.get_u $arr16_mut (global.get $g_arr16_mut) (local.get $1))
  )

  (func (export "array_init_data-null")
    (array.init_data $arr8_mut $d1 (ref.null $arr8_mut) (i32.const 0) (i32.const 0) (i32.const 0))
  )

  (func (export "array_init_data") (param $1 i32) (param $2 i32) (param $3 i32)
    (array.init_data $arr8_mut $d1 (global.get $g_arr8_mut) (local.get $1) (local.get $2) (local.get $3))
  )

  (func (export "array_init_data_i16") (param $1 i32) (param $2 i32) (param $3 i32)
    (array.init_data $arr16_mut $d1 (global.get $g_arr16_mut) (local.get $1) (local.get $2) (local.get $3))
  )

  (func (export "drop_segs")
    (data.drop $d1)
  )
)

;; null array argument traps
(assert_trap (invoke "array_init_data-null") "null array reference")

;; OOB initial index traps
(assert_trap (invoke "array_init_data" (i32.const 13) (i32.const 0) (i32.const 0)) "out of bounds array access")
(assert_trap (invoke "array_init_data" (i32.const 0) (i32.const 13) (i32.const 0)) "out of bounds memory access")

;; OOB length traps
(assert_trap (invoke "array_init_data" (i32.const 0) (i32.const 0) (i32.const 13)) "out of bounds array access")
(assert_trap (invoke "array_init_data" (i32.const 0) (i32.const 0) (i32.const 13)) "out of bounds array access")
(assert_trap (invoke "array_init_data_i16" (i32.const 0) (i32.const 0) (i32.const 7)) "out of bounds array access")

;; start index = array size, len = 0 doesn't trap
(assert_return (invoke "array_init_data" (i32.const 12) (i32.const 0) (i32.const 0)))
(assert_return (invoke "array_init_data" (i32.const 0) (i32.const 12) (i32.const 0)))
(assert_return (invoke "array_init_data_i16" (i32.const 0) (i32.const 6) (i32.const 0)))

;; check arrays were not modified
(assert_return (invoke "array_get_nth" (i32.const 0)) (i32.const 0))
(assert_return (invoke "array_get_nth" (i32.const 5)) (i32.const 0))
(assert_return (invoke "array_get_nth" (i32.const 11)) (i32.const 0))
(assert_trap (invoke "array_get_nth" (i32.const 12)) "out of bounds array access")
(assert_return (invoke "array_get_nth_i16" (i32.const 0)) (i32.const 0))
(assert_return (invoke "array_get_nth_i16" (i32.const 2)) (i32.const 0))
(assert_return (invoke "array_get_nth_i16" (i32.const 5)) (i32.const 0))
(assert_trap (invoke "array_get_nth_i16" (i32.const 6)) "out of bounds array access")

;; normal cases
(assert_return (invoke "array_init_data" (i32.const 4) (i32.const 2) (i32.const 2)))
(assert_return (invoke "array_get_nth" (i32.const 3)) (i32.const 0))
(assert_return (invoke "array_get_nth" (i32.const 4)) (i32.const 99))
(assert_return (invoke "array_get_nth" (i32.const 5)) (i32.const 100))
(assert_return (invoke "array_get_nth" (i32.const 6)) (i32.const 0))

(assert_return (invoke "array_init_data_i16" (i32.const 2) (i32.const 5) (i32.const 2)))
(assert_return (invoke "array_get_nth_i16" (i32.const 1)) (i32.const 0))
(assert_return (invoke "array_get_nth_i16" (i32.const 2)) (i32.const 0x6766))
(assert_return (invoke "array_get_nth_i16" (i32.const 3)) (i32.const 0x6968))
(assert_return (invoke "array_get_nth_i16" (i32.const 4)) (i32.const 0))

;; init_data/elem with dropped segments traps for non-zero length
(assert_return (invoke "drop_segs"))
(assert_return (invoke "array_init_data" (i32.const 0) (i32.const 0) (i32.const 0)))
(assert_trap (invoke "array_init_data" (i32.const 0) (i32.const 0) (i32.const 1)) "out of bounds memory access")
