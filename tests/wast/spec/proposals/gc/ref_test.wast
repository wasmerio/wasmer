;; Abstract Types

(module
  (type $ft (func))
  (type $st (struct))
  (type $at (array i8))

  (table $ta 10 anyref)
  (table $tf 10 funcref)
  (table $te 10 externref)

  (elem declare func $f)
  (func $f)

  (func (export "init") (param $x externref)
    (table.set $ta (i32.const 0) (ref.null any))
    (table.set $ta (i32.const 1) (ref.null struct))
    (table.set $ta (i32.const 2) (ref.null none))
    (table.set $ta (i32.const 3) (ref.i31 (i32.const 7)))
    (table.set $ta (i32.const 4) (struct.new_default $st))
    (table.set $ta (i32.const 5) (array.new_default $at (i32.const 0)))
    (table.set $ta (i32.const 6) (any.convert_extern (local.get $x)))
    (table.set $ta (i32.const 7) (any.convert_extern (ref.null extern)))

    (table.set $tf (i32.const 0) (ref.null nofunc))
    (table.set $tf (i32.const 1) (ref.null func))
    (table.set $tf (i32.const 2) (ref.func $f))

    (table.set $te (i32.const 0) (ref.null noextern))
    (table.set $te (i32.const 1) (ref.null extern))
    (table.set $te (i32.const 2) (local.get $x))
    (table.set $te (i32.const 3) (extern.convert_any (ref.i31 (i32.const 8))))
    (table.set $te (i32.const 4) (extern.convert_any (struct.new_default $st)))
    (table.set $te (i32.const 5) (extern.convert_any (ref.null any)))
  )

  (func (export "ref_test_null_data") (param $i i32) (result i32)
    (i32.add
      (ref.is_null (table.get $ta (local.get $i)))
      (ref.test nullref (table.get $ta (local.get $i)))
    )
  )
  (func (export "ref_test_any") (param $i i32) (result i32)
    (i32.add
      (ref.test (ref any) (table.get $ta (local.get $i)))
      (ref.test anyref (table.get $ta (local.get $i)))
    )
  )
  (func (export "ref_test_eq") (param $i i32) (result i32)
    (i32.add
      (ref.test (ref eq) (table.get $ta (local.get $i)))
      (ref.test eqref (table.get $ta (local.get $i)))
    )
  )
  (func (export "ref_test_i31") (param $i i32) (result i32)
    (i32.add
      (ref.test (ref i31) (table.get $ta (local.get $i)))
      (ref.test i31ref (table.get $ta (local.get $i)))
    )
  )
  (func (export "ref_test_struct") (param $i i32) (result i32)
    (i32.add
      (ref.test (ref struct) (table.get $ta (local.get $i)))
      (ref.test structref (table.get $ta (local.get $i)))
    )
  )
  (func (export "ref_test_array") (param $i i32) (result i32)
    (i32.add
      (ref.test (ref array) (table.get $ta (local.get $i)))
      (ref.test arrayref (table.get $ta (local.get $i)))
    )
  )

  (func (export "ref_test_null_func") (param $i i32) (result i32)
    (i32.add
      (ref.is_null (table.get $tf (local.get $i)))
      (ref.test (ref null nofunc) (table.get $tf (local.get $i)))
    )
  )
  (func (export "ref_test_func") (param $i i32) (result i32)
    (i32.add
      (ref.test (ref func) (table.get $tf (local.get $i)))
      (ref.test funcref (table.get $tf (local.get $i)))
    )
  )

  (func (export "ref_test_null_extern") (param $i i32) (result i32)
    (i32.add
      (ref.is_null (table.get $te (local.get $i)))
      (ref.test (ref null noextern) (table.get $te (local.get $i)))
    )
  )
  (func (export "ref_test_extern") (param $i i32) (result i32)
    (i32.add
      (ref.test (ref extern) (table.get $te (local.get $i)))
      (ref.test externref (table.get $te (local.get $i)))
    )
  )
)

(invoke "init" (ref.extern 0))

(assert_return (invoke "ref_test_null_data" (i32.const 0)) (i32.const 2))
(assert_return (invoke "ref_test_null_data" (i32.const 1)) (i32.const 2))
(assert_return (invoke "ref_test_null_data" (i32.const 2)) (i32.const 2))
(assert_return (invoke "ref_test_null_data" (i32.const 3)) (i32.const 0))
(assert_return (invoke "ref_test_null_data" (i32.const 4)) (i32.const 0))
(assert_return (invoke "ref_test_null_data" (i32.const 5)) (i32.const 0))
(assert_return (invoke "ref_test_null_data" (i32.const 6)) (i32.const 0))
(assert_return (invoke "ref_test_null_data" (i32.const 7)) (i32.const 2))

(assert_return (invoke "ref_test_any" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref_test_any" (i32.const 1)) (i32.const 1))
(assert_return (invoke "ref_test_any" (i32.const 2)) (i32.const 1))
(assert_return (invoke "ref_test_any" (i32.const 3)) (i32.const 2))
(assert_return (invoke "ref_test_any" (i32.const 4)) (i32.const 2))
(assert_return (invoke "ref_test_any" (i32.const 5)) (i32.const 2))
(assert_return (invoke "ref_test_any" (i32.const 6)) (i32.const 2))
(assert_return (invoke "ref_test_any" (i32.const 7)) (i32.const 1))

(assert_return (invoke "ref_test_eq" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref_test_eq" (i32.const 1)) (i32.const 1))
(assert_return (invoke "ref_test_eq" (i32.const 2)) (i32.const 1))
(assert_return (invoke "ref_test_eq" (i32.const 3)) (i32.const 2))
(assert_return (invoke "ref_test_eq" (i32.const 4)) (i32.const 2))
(assert_return (invoke "ref_test_eq" (i32.const 5)) (i32.const 2))
(assert_return (invoke "ref_test_eq" (i32.const 6)) (i32.const 0))
(assert_return (invoke "ref_test_eq" (i32.const 7)) (i32.const 1))

(assert_return (invoke "ref_test_i31" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref_test_i31" (i32.const 1)) (i32.const 1))
(assert_return (invoke "ref_test_i31" (i32.const 2)) (i32.const 1))
(assert_return (invoke "ref_test_i31" (i32.const 3)) (i32.const 2))
(assert_return (invoke "ref_test_i31" (i32.const 4)) (i32.const 0))
(assert_return (invoke "ref_test_i31" (i32.const 5)) (i32.const 0))
(assert_return (invoke "ref_test_i31" (i32.const 6)) (i32.const 0))
(assert_return (invoke "ref_test_i31" (i32.const 7)) (i32.const 1))

(assert_return (invoke "ref_test_struct" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref_test_struct" (i32.const 1)) (i32.const 1))
(assert_return (invoke "ref_test_struct" (i32.const 2)) (i32.const 1))
(assert_return (invoke "ref_test_struct" (i32.const 3)) (i32.const 0))
(assert_return (invoke "ref_test_struct" (i32.const 4)) (i32.const 2))
(assert_return (invoke "ref_test_struct" (i32.const 5)) (i32.const 0))
(assert_return (invoke "ref_test_struct" (i32.const 6)) (i32.const 0))
(assert_return (invoke "ref_test_struct" (i32.const 7)) (i32.const 1))

(assert_return (invoke "ref_test_array" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref_test_array" (i32.const 1)) (i32.const 1))
(assert_return (invoke "ref_test_array" (i32.const 2)) (i32.const 1))
(assert_return (invoke "ref_test_array" (i32.const 3)) (i32.const 0))
(assert_return (invoke "ref_test_array" (i32.const 4)) (i32.const 0))
(assert_return (invoke "ref_test_array" (i32.const 5)) (i32.const 2))
(assert_return (invoke "ref_test_array" (i32.const 6)) (i32.const 0))
(assert_return (invoke "ref_test_array" (i32.const 7)) (i32.const 1))

(assert_return (invoke "ref_test_null_func" (i32.const 0)) (i32.const 2))
(assert_return (invoke "ref_test_null_func" (i32.const 1)) (i32.const 2))
(assert_return (invoke "ref_test_null_func" (i32.const 2)) (i32.const 0))

(assert_return (invoke "ref_test_func" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref_test_func" (i32.const 1)) (i32.const 1))
(assert_return (invoke "ref_test_func" (i32.const 2)) (i32.const 2))

(assert_return (invoke "ref_test_null_extern" (i32.const 0)) (i32.const 2))
(assert_return (invoke "ref_test_null_extern" (i32.const 1)) (i32.const 2))
(assert_return (invoke "ref_test_null_extern" (i32.const 2)) (i32.const 0))
(assert_return (invoke "ref_test_null_extern" (i32.const 3)) (i32.const 0))
(assert_return (invoke "ref_test_null_extern" (i32.const 4)) (i32.const 0))
(assert_return (invoke "ref_test_null_extern" (i32.const 5)) (i32.const 2))

(assert_return (invoke "ref_test_extern" (i32.const 0)) (i32.const 1))
(assert_return (invoke "ref_test_extern" (i32.const 1)) (i32.const 1))
(assert_return (invoke "ref_test_extern" (i32.const 2)) (i32.const 2))
(assert_return (invoke "ref_test_extern" (i32.const 3)) (i32.const 2))
(assert_return (invoke "ref_test_extern" (i32.const 4)) (i32.const 2))
(assert_return (invoke "ref_test_extern" (i32.const 5)) (i32.const 1))


;; Concrete Types

(module
  (type $t0 (sub (struct)))
  (type $t1 (sub $t0 (struct (field i32))))
  (type $t1' (sub $t0 (struct (field i32))))
  (type $t2 (sub $t1 (struct (field i32 i32))))
  (type $t2' (sub $t1' (struct (field i32 i32))))
  (type $t3 (sub $t0 (struct (field i32 i32))))
  (type $t0' (sub $t0 (struct)))
  (type $t4 (sub $t0' (struct (field i32 i32))))

  (table 20 (ref null struct))

  (func $init
    (table.set (i32.const 0) (struct.new_default $t0))
    (table.set (i32.const 10) (struct.new_default $t0))
    (table.set (i32.const 1) (struct.new_default $t1))
    (table.set (i32.const 11) (struct.new_default $t1'))
    (table.set (i32.const 2) (struct.new_default $t2))
    (table.set (i32.const 12) (struct.new_default $t2'))
    (table.set (i32.const 3) (struct.new_default $t3))
    (table.set (i32.const 4) (struct.new_default $t4))
  )

  (func (export "test-sub")
    (call $init)
    (block $l
      ;; must hold
      (br_if $l (i32.eqz (ref.test (ref null $t0) (ref.null struct))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (ref.null $t0))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (ref.null $t1))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (ref.null $t2))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (ref.null $t3))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (ref.null $t4))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (table.get (i32.const 0)))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (table.get (i32.const 1)))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (table.get (i32.const 2)))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (table.get (i32.const 3)))))
      (br_if $l (i32.eqz (ref.test (ref null $t0) (table.get (i32.const 4)))))

      (br_if $l (i32.eqz (ref.test (ref null $t1) (ref.null struct))))
      (br_if $l (i32.eqz (ref.test (ref null $t1) (ref.null $t0))))
      (br_if $l (i32.eqz (ref.test (ref null $t1) (ref.null $t1))))
      (br_if $l (i32.eqz (ref.test (ref null $t1) (ref.null $t2))))
      (br_if $l (i32.eqz (ref.test (ref null $t1) (ref.null $t3))))
      (br_if $l (i32.eqz (ref.test (ref null $t1) (ref.null $t4))))
      (br_if $l (i32.eqz (ref.test (ref null $t1) (table.get (i32.const 1)))))
      (br_if $l (i32.eqz (ref.test (ref null $t1) (table.get (i32.const 2)))))

      (br_if $l (i32.eqz (ref.test (ref null $t2) (ref.null struct))))
      (br_if $l (i32.eqz (ref.test (ref null $t2) (ref.null $t0))))
      (br_if $l (i32.eqz (ref.test (ref null $t2) (ref.null $t1))))
      (br_if $l (i32.eqz (ref.test (ref null $t2) (ref.null $t2))))
      (br_if $l (i32.eqz (ref.test (ref null $t2) (ref.null $t3))))
      (br_if $l (i32.eqz (ref.test (ref null $t2) (ref.null $t4))))
      (br_if $l (i32.eqz (ref.test (ref null $t2) (table.get (i32.const 2)))))

      (br_if $l (i32.eqz (ref.test (ref null $t3) (ref.null struct))))
      (br_if $l (i32.eqz (ref.test (ref null $t3) (ref.null $t0))))
      (br_if $l (i32.eqz (ref.test (ref null $t3) (ref.null $t1))))
      (br_if $l (i32.eqz (ref.test (ref null $t3) (ref.null $t2))))
      (br_if $l (i32.eqz (ref.test (ref null $t3) (ref.null $t3))))
      (br_if $l (i32.eqz (ref.test (ref null $t3) (ref.null $t4))))
      (br_if $l (i32.eqz (ref.test (ref null $t3) (table.get (i32.const 3)))))

      (br_if $l (i32.eqz (ref.test (ref null $t4) (ref.null struct))))
      (br_if $l (i32.eqz (ref.test (ref null $t4) (ref.null $t0))))
      (br_if $l (i32.eqz (ref.test (ref null $t4) (ref.null $t1))))
      (br_if $l (i32.eqz (ref.test (ref null $t4) (ref.null $t2))))
      (br_if $l (i32.eqz (ref.test (ref null $t4) (ref.null $t3))))
      (br_if $l (i32.eqz (ref.test (ref null $t4) (ref.null $t4))))
      (br_if $l (i32.eqz (ref.test (ref null $t4) (table.get (i32.const 4)))))

      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 0)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 1)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 2)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 3)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 4)))))

      (br_if $l (i32.eqz (ref.test (ref $t1) (table.get (i32.const 1)))))
      (br_if $l (i32.eqz (ref.test (ref $t1) (table.get (i32.const 2)))))

      (br_if $l (i32.eqz (ref.test (ref $t2) (table.get (i32.const 2)))))

      (br_if $l (i32.eqz (ref.test (ref $t3) (table.get (i32.const 3)))))

      (br_if $l (i32.eqz (ref.test (ref $t4) (table.get (i32.const 4)))))

      ;; must not hold
      (br_if $l (ref.test (ref $t0) (ref.null struct)))
      (br_if $l (ref.test (ref $t1) (ref.null struct)))
      (br_if $l (ref.test (ref $t2) (ref.null struct)))
      (br_if $l (ref.test (ref $t3) (ref.null struct)))
      (br_if $l (ref.test (ref $t4) (ref.null struct)))

      (br_if $l (ref.test (ref $t1) (table.get (i32.const 0))))
      (br_if $l (ref.test (ref $t1) (table.get (i32.const 3))))
      (br_if $l (ref.test (ref $t1) (table.get (i32.const 4))))

      (br_if $l (ref.test (ref $t2) (table.get (i32.const 0))))
      (br_if $l (ref.test (ref $t2) (table.get (i32.const 1))))
      (br_if $l (ref.test (ref $t2) (table.get (i32.const 3))))
      (br_if $l (ref.test (ref $t2) (table.get (i32.const 4))))

      (br_if $l (ref.test (ref $t3) (table.get (i32.const 0))))
      (br_if $l (ref.test (ref $t3) (table.get (i32.const 1))))
      (br_if $l (ref.test (ref $t3) (table.get (i32.const 2))))
      (br_if $l (ref.test (ref $t3) (table.get (i32.const 4))))

      (br_if $l (ref.test (ref $t4) (table.get (i32.const 0))))
      (br_if $l (ref.test (ref $t4) (table.get (i32.const 1))))
      (br_if $l (ref.test (ref $t4) (table.get (i32.const 2))))
      (br_if $l (ref.test (ref $t4) (table.get (i32.const 3))))

      (return)
    )
    (unreachable)
  )

  (func (export "test-canon")
    (call $init)
    (block $l
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 0)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 1)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 2)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 3)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 4)))))

      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 10)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 11)))))
      (br_if $l (i32.eqz (ref.test (ref $t0) (table.get (i32.const 12)))))

      (br_if $l (i32.eqz (ref.test (ref $t1') (table.get (i32.const 1)))))
      (br_if $l (i32.eqz (ref.test (ref $t1') (table.get (i32.const 2)))))

      (br_if $l (i32.eqz (ref.test (ref $t1) (table.get (i32.const 11)))))
      (br_if $l (i32.eqz (ref.test (ref $t1) (table.get (i32.const 12)))))

      (br_if $l (i32.eqz (ref.test (ref $t2') (table.get (i32.const 2)))))

      (br_if $l (i32.eqz (ref.test (ref $t2) (table.get (i32.const 12)))))

      (return)
    )
    (unreachable)
  )
)

(assert_return (invoke "test-sub"))
(assert_return (invoke "test-canon"))
