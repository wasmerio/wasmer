;; Type syntax

(module
  (type (array i8))
  (type (array i16))
  (type (array i32))
  (type (array i64))
  (type (array f32))
  (type (array f64))
  (type (array anyref))
  (type (array (ref data)))
  (type (array (ref 0)))
  (type (array (ref null 1)))
  (type (array (rtt 1)))
  (type (array (rtt 10 1)))
  (type (array (mut i8)))
  (type (array (mut i16)))
  (type (array (mut i32)))
  (type (array (mut i64)))
  (type (array (mut i32)))
  (type (array (mut i64)))
  (type (array (mut anyref)))
  (type (array (mut (ref data))))
  (type (array (mut (ref 0))))
  (type (array (mut (ref null i31))))
  (type (array (mut (rtt 0))))
  (type (array (mut (rtt 10 0))))
)


(assert_invalid
  (module
    (type (array (mut (ref null 10))))
  )
  "unknown type"
)


;; Binding structure

(module
  (type $s0 (array (ref $s1)))
  (type $s1 (array (ref $s0)))

  (func (param (ref $forward)))

  (type $forward (array i32))
)

(assert_invalid
  (module (type (array (ref 1))))
  "unknown type"
)
(assert_invalid
  (module (type (array (mut (ref 1)))))
  "unknown type"
)


;; Basic instructions

(module
  (type $vec (array f32))
  (type $mvec (array (mut f32)))

  (func $get (param $i i32) (param $v (ref $vec)) (result f32)
    (array.get $vec (local.get $v) (local.get $i))
  )
  (func (export "get") (param $i i32) (result f32)
    (call $get (local.get $i)
      (array.new_default $vec (i32.const 3) (rtt.canon $vec))
    )
  )

  (func $set_get (param $i i32) (param $v (ref $mvec)) (param $y f32) (result f32)
    (array.set $mvec (local.get $v) (local.get $i) (local.get $y))
    (array.get $mvec (local.get $v) (local.get $i))
  )
  (func (export "set_get") (param $i i32) (param $y f32) (result f32)
    (call $set_get (local.get $i)
      (array.new_default $mvec (i32.const 3) (rtt.canon $mvec))
      (local.get $y)
    )
  )

  (func $len (param $v (ref $vec)) (result i32)
    (array.len $vec (local.get $v))
  )
  (func (export "len") (result i32)
    (call $len (array.new_default $vec (i32.const 3) (rtt.canon $vec)))
  )
)

(assert_return (invoke "get" (i32.const 0)) (f32.const 0))
(assert_return (invoke "set_get" (i32.const 1) (f32.const 7)) (f32.const 7))
(assert_return (invoke "len") (i32.const 3))

(assert_trap (invoke "get" (i32.const 10)) "out of bounds")
(assert_trap (invoke "set_get" (i32.const 10) (f32.const 7)) "out of bounds")

(assert_invalid
  (module
    (type $a (array i64))
    (func (export "array.set-immutable") (param $a (ref $a))
      (array.set $a (local.get $a) (i32.const 0) (i64.const 1))
    )
  )
  "array is immutable"
)


;; Null dereference

(module
  (type $t (array (mut i32)))
  (func (export "array.get-null")
    (local (ref null $t)) (drop (array.get $t (local.get 0) (i32.const 0)))
  )
  (func (export "array.set-null")
    (local (ref null $t)) (array.set $t (local.get 0) (i32.const 0) (i32.const 0))
  )
)

(assert_trap (invoke "array.get-null") "null array")
(assert_trap (invoke "array.set-null") "null array")

(assert_invalid
  (module
    (type $t (array i32))
    (func (export "array.new-null")
      (local (ref null (rtt $t))) (drop (array.new_default $t (i32.const 1) (i32.const 3) (local.get 0)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type $t (array (mut i32)))
    (func (export "array.new_default-null")
      (local (ref null (rtt $t))) (drop (array.new_default $t (i32.const 3) (local.get 0)))
    )
  )
  "type mismatch"
)
