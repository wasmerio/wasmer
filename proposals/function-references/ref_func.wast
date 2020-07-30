(module
  (func (export "f") (param $x i32) (result i32) (local.get $x))
)
(register "M")

(module
  (func $f (import "M" "f") (param i32) (result i32))
  (func $g (param $x i32) (result i32)
    (i32.add (local.get $x) (i32.const 1))
  )

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


;; Reference declaration

(module
  (func $f1)
  (func $f2)
  (func $f3)
  (func $f4)
  (func $f5)
  (func $f6)

  (table $t 1 funcref)

  (global funcref (ref.func $f1))
  (export "f" (func $f2))
  (elem (table $t) (i32.const 0) func $f3)
  (elem (table $t) (i32.const 0) funcref (ref.func $f4))
  (elem func $f5)
  (elem funcref (ref.func $f6))

  (func
    (ref.func $f1)
    (ref.func $f2)
    (ref.func $f3)
    (ref.func $f4)
    (ref.func $f5)
    (ref.func $f6)
    (return)
  )
)

(assert_invalid
  (module (func $f (drop (ref.func $f))))
  "undeclared function reference"
)
(assert_invalid
  (module (start $f) (func $f (drop (ref.func $f))))
  "undeclared function reference"
)
