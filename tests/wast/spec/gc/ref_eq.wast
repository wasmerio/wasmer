(module
  (type $st (sub (struct)))
  (type $st' (sub (struct (field i32))))
  (type $at (array i8))
  (type $st-sub1 (sub $st (struct)))
  (type $st-sub2 (sub $st (struct)))
  (type $st'-sub1 (sub $st' (struct (field i32))))
  (type $st'-sub2 (sub $st' (struct (field i32))))

  (table 20 (ref null eq))

  (func (export "init")
    (table.set (i32.const 0) (ref.null eq))
    (table.set (i32.const 1) (ref.null i31))
    (table.set (i32.const 2) (ref.i31 (i32.const 7)))
    (table.set (i32.const 3) (ref.i31 (i32.const 7)))
    (table.set (i32.const 4) (ref.i31 (i32.const 8)))
    (table.set (i32.const 5) (struct.new_default $st))
    (table.set (i32.const 6) (struct.new_default $st))
    (table.set (i32.const 7) (array.new_default $at (i32.const 0)))
    (table.set (i32.const 8) (array.new_default $at (i32.const 0)))
  )

  (func (export "eq") (param $i i32) (param $j i32) (result i32)
    (ref.eq (table.get (local.get $i)) (table.get (local.get $j)))
  )
)

(invoke "init")

(assert_return (invoke "eq" (i32.const 0) (i32.const 0)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 0) (i32.const 1)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 0) (i32.const 2)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0) (i32.const 3)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0) (i32.const 4)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0) (i32.const 5)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0) (i32.const 6)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0) (i32.const 7)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0) (i32.const 8)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 1) (i32.const 2)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 1) (i32.const 3)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 1) (i32.const 4)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 1) (i32.const 5)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 1) (i32.const 6)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 1) (i32.const 7)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 1) (i32.const 8)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 2) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 2) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 2) (i32.const 2)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 2) (i32.const 3)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 2) (i32.const 4)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 2) (i32.const 5)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 2) (i32.const 6)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 2) (i32.const 7)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 2) (i32.const 8)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 3) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 3) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 3) (i32.const 2)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 3) (i32.const 3)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 3) (i32.const 4)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 3) (i32.const 5)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 3) (i32.const 6)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 3) (i32.const 7)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 3) (i32.const 8)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 4) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 4) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 4) (i32.const 2)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 4) (i32.const 3)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 4) (i32.const 4)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 4) (i32.const 5)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 4) (i32.const 6)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 4) (i32.const 7)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 4) (i32.const 8)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 5) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 5) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 5) (i32.const 2)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 5) (i32.const 3)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 5) (i32.const 4)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 5) (i32.const 5)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 5) (i32.const 6)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 5) (i32.const 7)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 5) (i32.const 8)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 6) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 6) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 6) (i32.const 2)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 6) (i32.const 3)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 6) (i32.const 4)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 6) (i32.const 5)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 6) (i32.const 6)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 6) (i32.const 7)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 6) (i32.const 8)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 7) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 7) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 7) (i32.const 2)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 7) (i32.const 3)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 7) (i32.const 4)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 7) (i32.const 5)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 7) (i32.const 6)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 7) (i32.const 7)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 7) (i32.const 8)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 8) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 8) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 8) (i32.const 2)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 8) (i32.const 3)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 8) (i32.const 4)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 8) (i32.const 5)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 8) (i32.const 6)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 8) (i32.const 7)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 8) (i32.const 8)) (i32.const 1))

(assert_invalid
  (module
    (func (export "eq") (param $r (ref any)) (result i32)
      (ref.eq (local.get $r) (local.get $r))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func (export "eq") (param $r (ref null any)) (result i32)
      (ref.eq (local.get $r) (local.get $r))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func (export "eq") (param $r (ref func)) (result i32)
      (ref.eq (local.get $r) (local.get $r))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func (export "eq") (param $r (ref null func)) (result i32)
      (ref.eq (local.get $r) (local.get $r))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func (export "eq") (param $r (ref extern)) (result i32)
      (ref.eq (local.get $r) (local.get $r))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func (export "eq") (param $r (ref null extern)) (result i32)
      (ref.eq (local.get $r) (local.get $r))
    )
  )
  "type mismatch"
)
