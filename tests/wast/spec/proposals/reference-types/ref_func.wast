(module
  (func (export "f") (param $x i32) (result i32) (local.get $x))
)
(register "M")

(module
  (func $f (import "M" "f") (param i32) (result i32))
  (func $g (param $x i32) (result i32)
    (i32.add (local.get $x) (i32.const 1))
  )

  (global anyref (ref.func $f))
  (global anyref (ref.func $g))
  (global funcref (ref.func $f))
  (global funcref (ref.func $g))
  (global $v (mut funcref) (ref.func $f))

  (global funcref (ref.func $gf1))
  (global funcref (ref.func $gf2))
  (func (drop (ref.func $ff1)) (drop (ref.func $ff2)))
  (elem declare func $gf1 $ff1)
  (elem declare funcref (ref.func $gf2) (ref.func $ff2))
  (func $gf1)
  (func $gf2)
  (func $ff1)
  (func $ff2)

  (func (export "is_null-f") (result i32)
    (ref.is_null (ref.func $f))
  )
  (func (export "is_null-g") (result i32)
    (ref.is_null (ref.func $g))
  )
  (func (export "is_null-v") (result i32)
    (ref.is_null (global.get $v))
  )

  (func (export "set-f") (global.set $v (ref.func $f)))
  (func (export "set-g") (global.set $v (ref.func $g)))

  (table $t 1 funcref)
  (elem declare func $f $g)

  (func (export "call-f") (param $x i32) (result i32)
    (table.set $t (i32.const 0) (ref.func $f))
    (call_indirect $t (param i32) (result i32) (local.get $x) (i32.const 0))
  )
  (func (export "call-g") (param $x i32) (result i32)
    (table.set $t (i32.const 0) (ref.func $g))
    (call_indirect $t (param i32) (result i32) (local.get $x) (i32.const 0))
  )
  (func (export "call-v") (param $x i32) (result i32)
    (table.set $t (i32.const 0) (global.get $v))
    (call_indirect $t (param i32) (result i32) (local.get $x) (i32.const 0))
  )
)

(assert_return (invoke "is_null-f") (i32.const 0))
(assert_return (invoke "is_null-g") (i32.const 0))
(assert_return (invoke "is_null-v") (i32.const 0))

(assert_return (invoke "call-f" (i32.const 4)) (i32.const 4))
(assert_return (invoke "call-g" (i32.const 4)) (i32.const 5))
(assert_return (invoke "call-v" (i32.const 4)) (i32.const 4))
(invoke "set-g")
(assert_return (invoke "call-v" (i32.const 4)) (i32.const 5))
(invoke "set-f")
(assert_return (invoke "call-v" (i32.const 4)) (i32.const 4))

(assert_invalid
  (module
    (func $f (import "M" "f") (param i32) (result i32))
    (func $g (import "M" "g") (param i32) (result i32))
    (global funcref (ref.func 7))
  )
  "unknown function 7"
)

(assert_invalid
  (module (func $f) (global funcref (ref.func $f)))
  "undeclared function reference"
)
(assert_invalid
  (module (func $f (drop (ref.func $f))))
  "undeclared function reference"
)
