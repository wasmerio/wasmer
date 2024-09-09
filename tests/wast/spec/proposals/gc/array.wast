;; Type syntax

(module
  (type (array i8))
  (type (array i16))
  (type (array i32))
  (type (array i64))
  (type (array f32))
  (type (array f64))
  (type (array anyref))
  (type (array (ref struct)))
  (type (array (ref 0)))
  (type (array (ref null 1)))
  (type (array (mut i8)))
  (type (array (mut i16)))
  (type (array (mut i32)))
  (type (array (mut i64)))
  (type (array (mut i32)))
  (type (array (mut i64)))
  (type (array (mut anyref)))
  (type (array (mut (ref struct))))
  (type (array (mut (ref 0))))
  (type (array (mut (ref null i31))))
)


(assert_invalid
  (module
    (type (array (mut (ref null 10))))
  )
  "unknown type"
)


;; Binding structure

(module
  (rec
    (type $s0 (array (ref $s1)))
    (type $s1 (array (ref $s0)))
  )

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

  (global (ref $vec) (array.new $vec (f32.const 1) (i32.const 3)))
  (global (ref $vec) (array.new_default $vec (i32.const 3)))

  (func $new (export "new") (result (ref $vec))
    (array.new_default $vec (i32.const 3))
  )

  (func $get (param $i i32) (param $v (ref $vec)) (result f32)
    (array.get $vec (local.get $v) (local.get $i))
  )
  (func (export "get") (param $i i32) (result f32)
    (call $get (local.get $i) (call $new))
  )

  (func $set_get (param $i i32) (param $v (ref $mvec)) (param $y f32) (result f32)
    (array.set $mvec (local.get $v) (local.get $i) (local.get $y))
    (array.get $mvec (local.get $v) (local.get $i))
  )
  (func (export "set_get") (param $i i32) (param $y f32) (result f32)
    (call $set_get (local.get $i)
      (array.new_default $mvec (i32.const 3))
      (local.get $y)
    )
  )

  (func $len (param $v (ref array)) (result i32)
    (array.len (local.get $v))
  )
  (func (export "len") (result i32)
    (call $len (call $new))
  )
)

(assert_return (invoke "new") (ref.array))
(assert_return (invoke "new") (ref.eq))
(assert_return (invoke "get" (i32.const 0)) (f32.const 0))
(assert_return (invoke "set_get" (i32.const 1) (f32.const 7)) (f32.const 7))
(assert_return (invoke "len") (i32.const 3))

(assert_trap (invoke "get" (i32.const 10)) "out of bounds array access")
(assert_trap (invoke "set_get" (i32.const 10) (f32.const 7)) "out of bounds array access")

(module
  (type $vec (array f32))
  (type $mvec (array (mut f32)))

  (global (ref $vec) (array.new_fixed $vec 2 (f32.const 1) (f32.const 2)))

  (func $new (export "new") (result (ref $vec))
    (array.new_fixed $vec 2 (f32.const 1) (f32.const 2))
  )

  (func $get (param $i i32) (param $v (ref $vec)) (result f32)
    (array.get $vec (local.get $v) (local.get $i))
  )
  (func (export "get") (param $i i32) (result f32)
    (call $get (local.get $i) (call $new))
  )

  (func $set_get (param $i i32) (param $v (ref $mvec)) (param $y f32) (result f32)
    (array.set $mvec (local.get $v) (local.get $i) (local.get $y))
    (array.get $mvec (local.get $v) (local.get $i))
  )
  (func (export "set_get") (param $i i32) (param $y f32) (result f32)
    (call $set_get (local.get $i)
      (array.new_fixed $mvec 3 (f32.const 1) (f32.const 2) (f32.const 3))
      (local.get $y)
    )
  )

  (func $len (param $v (ref array)) (result i32)
    (array.len (local.get $v))
  )
  (func (export "len") (result i32)
    (call $len (call $new))
  )
)

(assert_return (invoke "new") (ref.array))
(assert_return (invoke "new") (ref.eq))
(assert_return (invoke "get" (i32.const 0)) (f32.const 1))
(assert_return (invoke "set_get" (i32.const 1) (f32.const 7)) (f32.const 7))
(assert_return (invoke "len") (i32.const 2))

(assert_trap (invoke "get" (i32.const 10)) "out of bounds array access")
(assert_trap (invoke "set_get" (i32.const 10) (f32.const 7)) "out of bounds array access")

(module
  (type $vec (array i8))
  (type $mvec (array (mut i8)))

  (data $d "\00\01\02\ff\04")

  (func $new (export "new") (result (ref $vec))
    (array.new_data $vec $d (i32.const 1) (i32.const 3))
  )

  (func $get_u (param $i i32) (param $v (ref $vec)) (result i32)
    (array.get_u $vec (local.get $v) (local.get $i))
  )
  (func (export "get_u") (param $i i32) (result i32)
    (call $get_u (local.get $i) (call $new))
  )

  (func $get_s (param $i i32) (param $v (ref $vec)) (result i32)
    (array.get_s $vec (local.get $v) (local.get $i))
  )
  (func (export "get_s") (param $i i32) (result i32)
    (call $get_s (local.get $i) (call $new))
  )

  (func $set_get (param $i i32) (param $v (ref $mvec)) (param $y i32) (result i32)
    (array.set $mvec (local.get $v) (local.get $i) (local.get $y))
    (array.get_u $mvec (local.get $v) (local.get $i))
  )
  (func (export "set_get") (param $i i32) (param $y i32) (result i32)
    (call $set_get (local.get $i)
      (array.new_data $mvec $d (i32.const 1) (i32.const 3))
      (local.get $y)
    )
  )

  (func $len (param $v (ref array)) (result i32)
    (array.len (local.get $v))
  )
  (func (export "len") (result i32)
    (call $len (call $new))
  )
)

(assert_return (invoke "new") (ref.array))
(assert_return (invoke "new") (ref.eq))
(assert_return (invoke "get_u" (i32.const 2)) (i32.const 0xff))
(assert_return (invoke "get_s" (i32.const 2)) (i32.const -1))
(assert_return (invoke "set_get" (i32.const 1) (i32.const 7)) (i32.const 7))
(assert_return (invoke "len") (i32.const 3))

(assert_trap (invoke "get_u" (i32.const 10)) "out of bounds array access")
(assert_trap (invoke "get_s" (i32.const 10)) "out of bounds array access")
(assert_trap (invoke "set_get" (i32.const 10) (i32.const 7)) "out of bounds array access")

(module
  (type $bvec (array i8))
  (type $vec (array (ref $bvec)))
  (type $mvec (array (mut (ref $bvec))))
  (type $nvec (array (ref null $bvec)))
  (type $avec (array (mut anyref)))

  (elem $e (ref $bvec)
    (array.new $bvec (i32.const 7) (i32.const 3))
    (array.new_fixed $bvec 2 (i32.const 1) (i32.const 2))
  )

  (func $new (export "new") (result (ref $vec))
    (array.new_elem $vec $e (i32.const 0) (i32.const 2))
  )

  (func $sub1 (result (ref $nvec))
    (array.new_elem $nvec $e (i32.const 0) (i32.const 2))
  )
  (func $sub2 (result (ref $avec))
    (array.new_elem $avec $e (i32.const 0) (i32.const 2))
  )

  (func $get (param $i i32) (param $j i32) (param $v (ref $vec)) (result i32)
    (array.get_u $bvec (array.get $vec (local.get $v) (local.get $i)) (local.get $j))
  )
  (func (export "get") (param $i i32) (param $j i32) (result i32)
    (call $get (local.get $i) (local.get $j) (call $new))
  )

  (func $set_get (param $i i32) (param $j i32) (param $v (ref $mvec)) (param $y i32) (result i32)
    (array.set $mvec (local.get $v) (local.get $i) (array.get $mvec (local.get $v) (local.get $y)))
    (array.get_u $bvec (array.get $mvec (local.get $v) (local.get $i)) (local.get $j))
  )
  (func (export "set_get") (param $i i32) (param $j i32) (param $y i32) (result i32)
    (call $set_get (local.get $i) (local.get $j)
      (array.new_elem $mvec $e (i32.const 0) (i32.const 2))
      (local.get $y)
    )
  )

  (func $len (param $v (ref array)) (result i32)
    (array.len (local.get $v))
  )
  (func (export "len") (result i32)
    (call $len (call $new))
  )
)

(assert_return (invoke "new") (ref.array))
(assert_return (invoke "new") (ref.eq))
(assert_return (invoke "get" (i32.const 0) (i32.const 0)) (i32.const 7))
(assert_return (invoke "get" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "set_get" (i32.const 0) (i32.const 1) (i32.const 1)) (i32.const 2))
(assert_return (invoke "len") (i32.const 2))

(assert_trap (invoke "get" (i32.const 10) (i32.const 0)) "out of bounds array access")
(assert_trap (invoke "set_get" (i32.const 10) (i32.const 0) (i32.const 0)) "out of bounds array access")

(assert_invalid
  (module
    (type $a (array i64))
    (func (export "array.set-immutable") (param $a (ref $a))
      (array.set $a (local.get $a) (i32.const 0) (i64.const 1))
    )
  )
  "array is immutable"
)

(assert_invalid
  (module
    (type $bvec (array i8))

    (data $d "\00\01\02\03\04")

    (global (ref $bvec)
      (array.new_data $bvec $d (i32.const 1) (i32.const 3))
    )
  )
  "constant expression required"
)

(assert_invalid
  (module
    (type $bvec (array i8))
    (type $vvec (array (ref $bvec)))

    (elem $e (ref $bvec) (ref.null $bvec))

    (global (ref $vvec)
      (array.new_elem $vvec $e (i32.const 0) (i32.const 1))
    )
  )
  "constant expression required"
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

(assert_trap (invoke "array.get-null") "null array reference")
(assert_trap (invoke "array.set-null") "null array reference")
