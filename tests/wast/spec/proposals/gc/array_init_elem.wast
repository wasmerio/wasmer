;; Bulk instructions

;; invalid uses

(assert_invalid
  (module
    (type $a (array funcref))

    (elem $e1 funcref)

    (func (export "array.init_elem-immutable") (param $1 (ref $a))
      (array.init_elem $a $e1 (local.get $1) (i32.const 0) (i32.const 0) (i32.const 0))
    )
  )
  "array is immutable"
)

(assert_invalid
  (module
    (type $a (array (mut i8)))

    (elem $e1 funcref)

    (func (export "array.init_elem-invalid-1") (param $1 (ref $a))
      (array.init_elem $a $e1 (local.get $1) (i32.const 0) (i32.const 0) (i32.const 0))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (type $a (array (mut funcref)))

    (elem $e1 externref)

    (func (export "array.init_elem-invalid-2") (param $1 (ref $a))
      (array.init_elem $a $e1 (local.get $1) (i32.const 0) (i32.const 0) (i32.const 0))
    )
  )
  "type mismatch"
)

(module
  (type $t_f (func))
  (type $arrref (array (ref $t_f)))
  (type $arrref_mut (array (mut funcref)))

  (global $g_arrref (ref $arrref) (array.new $arrref (ref.func $dummy) (i32.const 12)))
  (global $g_arrref_mut (ref $arrref_mut) (array.new_default $arrref_mut (i32.const 12)))

  (table $t 1 funcref)

  (elem $e1 func $dummy $dummy $dummy $dummy $dummy $dummy $dummy $dummy $dummy $dummy $dummy $dummy)

  (func $dummy
  )

  (func (export "array_call_nth") (param $1 i32)
    (table.set $t (i32.const 0) (array.get $arrref_mut (global.get $g_arrref_mut) (local.get $1)))
    (call_indirect $t (i32.const 0))
  )

  (func (export "array_init_elem-null")
    (array.init_elem $arrref_mut $e1 (ref.null $arrref_mut) (i32.const 0) (i32.const 0) (i32.const 0))
  )

  (func (export "array_init_elem") (param $1 i32) (param $2 i32) (param $3 i32)
    (array.init_elem $arrref_mut $e1 (global.get $g_arrref_mut) (local.get $1) (local.get $2) (local.get $3))
  )

  (func (export "drop_segs")
    (elem.drop $e1)
  )
)

;; null array argument traps
(assert_trap (invoke "array_init_elem-null") "null array reference")

;; OOB initial index traps
(assert_trap (invoke "array_init_elem" (i32.const 13) (i32.const 0) (i32.const 0)) "out of bounds array access")
(assert_trap (invoke "array_init_elem" (i32.const 0) (i32.const 13) (i32.const 0)) "out of bounds table access")

;; OOB length traps
(assert_trap (invoke "array_init_elem" (i32.const 0) (i32.const 0) (i32.const 13)) "out of bounds array access")
(assert_trap (invoke "array_init_elem" (i32.const 0) (i32.const 0) (i32.const 13)) "out of bounds array access")

;; start index = array size, len = 0 doesn't trap
(assert_return (invoke "array_init_elem" (i32.const 12) (i32.const 0) (i32.const 0)))
(assert_return (invoke "array_init_elem" (i32.const 0) (i32.const 12) (i32.const 0)))

;; check arrays were not modified
(assert_trap (invoke "array_call_nth" (i32.const 0)) "uninitialized element")
(assert_trap (invoke "array_call_nth" (i32.const 5)) "uninitialized element")
(assert_trap (invoke "array_call_nth" (i32.const 11)) "uninitialized element")
(assert_trap (invoke "array_call_nth" (i32.const 12)) "out of bounds array access")

;; normal cases
(assert_return (invoke "array_init_elem" (i32.const 2) (i32.const 3) (i32.const 2)))
(assert_trap (invoke "array_call_nth" (i32.const 1)) "uninitialized element")
(assert_return (invoke "array_call_nth" (i32.const 2)))
(assert_return (invoke "array_call_nth" (i32.const 3)))
(assert_trap (invoke "array_call_nth" (i32.const 4)) "uninitialized element")

;; init_data/elem with dropped segments traps for non-zero length
(assert_return (invoke "drop_segs"))
(assert_return (invoke "array_init_elem" (i32.const 0) (i32.const 0) (i32.const 0)))
(assert_trap (invoke "array_init_elem" (i32.const 0) (i32.const 0) (i32.const 1)) "out of bounds table access")
