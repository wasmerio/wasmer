(module
  (func $f1 (export "funcref") (param $x funcref) (result i32)
    (ref.is_null (local.get $x))
  )
  (func $f2 (export "externref") (param $x externref) (result i32)
    (ref.is_null (local.get $x))
  )

  (table $t1 2 funcref)
  (table $t2 2 externref)
  (elem (table $t1) (i32.const 1) func $dummy)
  (func $dummy)

  (func (export "init") (param $r externref)
    (table.set $t2 (i32.const 1) (local.get $r))
  )
  (func (export "deinit")
    (table.set $t1 (i32.const 1) (ref.null func))
    (table.set $t2 (i32.const 1) (ref.null extern))
  )

  (func (export "funcref-elem") (param $x i32) (result i32)
    (call $f1 (table.get $t1 (local.get $x)))
  )
  (func (export "externref-elem") (param $x i32) (result i32)
    (call $f2 (table.get $t2 (local.get $x)))
  )
)

(assert_return (invoke "funcref" (ref.null func)) (i32.const 1))
(assert_return (invoke "externref" (ref.null extern)) (i32.const 1))

(assert_return (invoke "externref" (ref.extern 1)) (i32.const 0))

(invoke "init" (ref.extern 0))

(assert_return (invoke "funcref-elem" (i32.const 0)) (i32.const 1))
(assert_return (invoke "externref-elem" (i32.const 0)) (i32.const 1))

(assert_return (invoke "funcref-elem" (i32.const 1)) (i32.const 0))
(assert_return (invoke "externref-elem" (i32.const 1)) (i32.const 0))

(invoke "deinit")

(assert_return (invoke "funcref-elem" (i32.const 0)) (i32.const 1))
(assert_return (invoke "externref-elem" (i32.const 0)) (i32.const 1))

(assert_return (invoke "funcref-elem" (i32.const 1)) (i32.const 1))
(assert_return (invoke "externref-elem" (i32.const 1)) (i32.const 1))

(assert_invalid
  (module (func $ref-vs-num (param i32) (ref.is_null (local.get 0))))
  "type mismatch"
)
(assert_invalid
  (module (func $ref-vs-empty (ref.is_null)))
  "type mismatch"
)
