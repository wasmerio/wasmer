(module
  (type $t (func (result i32)))

  (func $nn (param $r (ref $t)) (result i32)
    (block $l
      (return (call_ref (br_on_null $l (local.get $r))))
    )
    (i32.const -1)
  )
  (func $n (param $r (ref null $t)) (result i32)
    (block $l
      (return (call_ref (br_on_null $l (local.get $r))))
    )
    (i32.const -1)
  )

  (elem func $f)
  (func $f (result i32) (i32.const 7))

  (func (export "nullable-null") (result i32) (call $n (ref.null $t)))
  (func (export "nonnullable-f") (result i32) (call $nn (ref.func $f)))
  (func (export "nullable-f") (result i32) (call $n (ref.func $f)))

  (func (export "unreachable") (result i32)
    (block $l
      (return (call_ref (br_on_null $l (unreachable))))
    )
    (i32.const -1)
  )
)

(assert_trap (invoke "unreachable") "unreachable")

(assert_return (invoke "nullable-null") (i32.const -1))
(assert_return (invoke "nonnullable-f") (i32.const 7))
(assert_return (invoke "nullable-f") (i32.const 7))

(assert_invalid
  (module
    (type $t (func (result i32)))
    (func $g (param $r (ref $t)) (drop (br_on_null 0 (local.get $r))))
    (func (call $g (ref.null $t)))
  )
  "type mismatch"
)

(module
  (type $t (func))
  (func (param $r (ref $t)) (drop (br_on_null 0 (local.get $r))))
  (func (param $r (ref func)) (drop (br_on_null 0 (local.get $r))))
  (func (param $r (ref extern)) (drop (br_on_null 0 (local.get $r))))
)


(module
  (type $t (func (param i32) (result i32)))
  (elem func $f)
  (func $f (param i32) (result i32) (i32.mul (local.get 0) (local.get 0)))

  (func $a (param $n i32) (param $r (ref null $t)) (result i32)
    (block $l (result i32)
      (return (call_ref (br_on_null $l (local.get $n) (local.get $r))))
    )
  )

  (func (export "args-null") (param $n i32) (result i32)
    (call $a (local.get $n) (ref.null $t))
  )
  (func (export "args-f") (param $n i32) (result i32)
    (call $a (local.get $n) (ref.func $f))
  )
)

(assert_return (invoke "args-null" (i32.const 3)) (i32.const 3))
(assert_return (invoke "args-f" (i32.const 3)) (i32.const 9))
