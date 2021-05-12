;; Type syntax

(module
  (type (struct))
  (type (struct (field)))
  (type (struct (field i8)))
  (type (struct (field i8 i8 i8 i8)))
  (type (struct (field $x1 i32) (field $y1 i32)))
  (type (struct (field i8 i16 i32 i64 f32 f64 anyref funcref (ref 0) (ref null 1))))
  (type (struct (field i32 i64 i8) (field) (field) (field (ref null i31) anyref)))
  (type (struct (field $x2 i32) (field f32 f64) (field $y2 i32)))
)


(assert_malformed
  (module quote
    "(type (struct (field $x i32) (field $x i32)))"
  )
  "duplicate field"
)
(assert_malformed
  (module quote
    "(type (struct (field $x i32)))"
    "(type (struct (field $x i32)))"
  )
  "duplicate field"
)


;; Binding structure

(module
  (type $s0 (struct (field (ref 0) (ref 1) (ref $s0) (ref $s1))))
  (type $s1 (struct (field (ref 0) (ref 1) (ref $s0) (ref $s1))))

  (func (param (ref $forward)))

  (type $forward (struct))
)

(assert_invalid
  (module (type (struct (field (ref 1)))))
  "unknown type"
)
(assert_invalid
  (module (type (struct (field (mut (ref 1))))))
  "unknown type"
)


;; Basic instructions

(module
  (type $vec (struct (field f32) (field $y (mut f32)) (field $z f32)))

  (func $get_0 (param $v (ref $vec)) (result f32)
    (struct.get $vec 0 (local.get $v))
  )
  (func (export "get_0") (result f32)
    (call $get_0 (struct.new_default $vec (rtt.canon $vec)))
  )

  (func $set_get_y (param $v (ref $vec)) (param $y f32) (result f32)
    (struct.set $vec $y (local.get $v) (local.get $y))
    (struct.get $vec $y (local.get $v))
  )
  (func (export "set_get_y") (param $y f32) (result f32)
    (call $set_get_y (struct.new_default $vec (rtt.canon $vec)) (local.get $y))
  )

  (func $set_get_1 (param $v (ref $vec)) (param $y f32) (result f32)
    (struct.set $vec 1 (local.get $v) (local.get $y))
    (struct.get $vec $y (local.get $v))
  )
  (func (export "set_get_1") (param $y f32) (result f32)
    (call $set_get_1 (struct.new_default $vec (rtt.canon $vec)) (local.get $y))
  )
)

(assert_return (invoke "get_0") (f32.const 0))
(assert_return (invoke "set_get_y" (f32.const 7)) (f32.const 7))
(assert_return (invoke "set_get_1" (f32.const 7)) (f32.const 7))

(assert_invalid
  (module
    (type $s (struct (field i64)))
    (func (export "struct.set-immutable") (param $s (ref $s))
      (struct.set $s 0 (local.get $s) (i64.const 1))
    )
  )
  "field is immutable"
)


;; Null dereference

(module
  (type $t (struct (field i32 (mut i32))))
  (func (export "struct.get-null")
    (local (ref null $t)) (drop (struct.get $t 1 (local.get 0)))
  )
  (func (export "struct.set-null")
    (local (ref null $t)) (struct.set $t 1 (local.get 0) (i32.const 0))
  )
)

(assert_trap (invoke "struct.get-null") "null structure")
(assert_trap (invoke "struct.set-null") "null structure")

(assert_invalid
  (module
    (type $t (struct (field i32 (mut i32))))
    (func (export "struct.new-null")
      (local (ref null (rtt $t))) (drop (struct.new $t (i32.const 1) (i32.const 2) (local.get 0)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type $t (struct (field i32 (mut i32))))
    (func (export "struct.new_default-null")
      (local (ref null (rtt $t))) (drop (struct.new_default $t (local.get 0)))
    )
  )
  "type mismatch"
)
