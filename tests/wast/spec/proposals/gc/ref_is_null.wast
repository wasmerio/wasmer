(module
  (type $t (func))
  (func $dummy)

  (func $f1 (export "funcref") (param $x funcref) (result i32)
    (ref.is_null (local.get $x))
  )
  (func $f2 (export "externref") (param $x externref) (result i32)
    (ref.is_null (local.get $x))
  )
  (func $f3 (param $x (ref null $t)) (result i32)
    (ref.is_null (local.get $x))
  )
  (func $f3' (export "ref-null") (result i32)
    (call $f3 (ref.null $t))
  )

  (table $t1 2 funcref)
  (table $t2 2 externref)
  (table $t3 2 (ref null $t))
  (elem (table $t1) (i32.const 1) func $dummy)
  (elem (table $t3) (i32.const 1) (ref $t) (ref.func $dummy))

  (func (export "init") (param $r externref)
    (table.set $t2 (i32.const 1) (local.get $r))
  )
  (func (export "deinit")
    (table.set $t1 (i32.const 1) (ref.null func))
    (table.set $t2 (i32.const 1) (ref.null extern))
    (table.set $t3 (i32.const 1) (ref.null $t))
  )

  (func (export "funcref-elem") (param $x i32) (result i32)
    (call $f1 (table.get $t1 (local.get $x)))
  )
  (func (export "externref-elem") (param $x i32) (result i32)
    (call $f2 (table.get $t2 (local.get $x)))
  )
  (func (export "ref-elem") (param $x i32) (result i32)
    (call $f3 (table.get $t3 (local.get $x)))
  )
)

(assert_return (invoke "funcref" (ref.null func)) (i32.const 1))
(assert_return (invoke "externref" (ref.null extern)) (i32.const 1))
(assert_return (invoke "ref-null") (i32.const 1))

(assert_return (invoke "externref" (ref.extern 1)) (i32.const 0))

(invoke "init" (ref.extern 0))

(assert_return (invoke "funcref-elem" (i32.const 0)) (i32.const 1))
(assert_return (invoke "externref-elem" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref-elem" (i32.const 0)) (i32.const 1))

(assert_return (invoke "funcref-elem" (i32.const 1)) (i32.const 0))
(assert_return (invoke "externref-elem" (i32.const 1)) (i32.const 0))
(assert_return (invoke "ref-elem" (i32.const 1)) (i32.const 0))

(invoke "deinit")

(assert_return (invoke "funcref-elem" (i32.const 0)) (i32.const 1))
(assert_return (invoke "externref-elem" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref-elem" (i32.const 0)) (i32.const 1))

(assert_return (invoke "funcref-elem" (i32.const 1)) (i32.const 1))
(assert_return (invoke "externref-elem" (i32.const 1)) (i32.const 1))
(assert_return (invoke "ref-elem" (i32.const 1)) (i32.const 1))


(module
  (type $t (func))
  (func (param $r (ref $t)) (drop (ref.is_null (local.get $r))))
  (func (param $r (ref func)) (drop (ref.is_null (local.get $r))))
  (func (param $r (ref extern)) (drop (ref.is_null (local.get $r))))
)

(assert_invalid
  (module (func $ref-vs-num (param i32) (ref.is_null (local.get 0))))
  "type mismatch"
)
(assert_invalid
  (module (func $ref-vs-empty (ref.is_null)))
  "type mismatch"
)
