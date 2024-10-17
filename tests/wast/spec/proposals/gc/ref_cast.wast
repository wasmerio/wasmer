;; Abstract Types

(module
  (type $ft (func))
  (type $st (struct))
  (type $at (array i8))

  (table 10 anyref)

  (elem declare func $f)
  (func $f)

  (func (export "init") (param $x externref)
    (table.set (i32.const 0) (ref.null any))
    (table.set (i32.const 1) (ref.i31 (i32.const 7)))
    (table.set (i32.const 2) (struct.new_default $st))
    (table.set (i32.const 3) (array.new_default $at (i32.const 0)))
    (table.set (i32.const 4) (any.convert_extern (local.get $x)))
    (table.set (i32.const 5) (ref.null i31))
    (table.set (i32.const 6) (ref.null struct))
    (table.set (i32.const 7) (ref.null none))
  )

  (func (export "ref_cast_non_null") (param $i i32)
    (drop (ref.as_non_null (table.get (local.get $i))))
    (drop (ref.cast (ref null any) (table.get (local.get $i))))
  )
  (func (export "ref_cast_null") (param $i i32)
    (drop (ref.cast anyref (table.get (local.get $i))))
    (drop (ref.cast structref (table.get (local.get $i))))
    (drop (ref.cast arrayref (table.get (local.get $i))))
    (drop (ref.cast i31ref (table.get (local.get $i))))
    (drop (ref.cast nullref (table.get (local.get $i))))
  )
  (func (export "ref_cast_i31") (param $i i32)
    (drop (ref.cast (ref i31) (table.get (local.get $i))))
    (drop (ref.cast i31ref (table.get (local.get $i))))
  )
  (func (export "ref_cast_struct") (param $i i32)
    (drop (ref.cast (ref struct) (table.get (local.get $i))))
    (drop (ref.cast structref (table.get (local.get $i))))
  )
  (func (export "ref_cast_array") (param $i i32)
    (drop (ref.cast (ref array) (table.get (local.get $i))))
    (drop (ref.cast arrayref (table.get (local.get $i))))
  )
)

(invoke "init" (ref.extern 0))

(assert_trap (invoke "ref_cast_non_null" (i32.const 0)) "null reference")
(assert_return (invoke "ref_cast_non_null" (i32.const 1)))
(assert_return (invoke "ref_cast_non_null" (i32.const 2)))
(assert_return (invoke "ref_cast_non_null" (i32.const 3)))
(assert_return (invoke "ref_cast_non_null" (i32.const 4)))
(assert_trap (invoke "ref_cast_non_null" (i32.const 5)) "null reference")
(assert_trap (invoke "ref_cast_non_null" (i32.const 6)) "null reference")
(assert_trap (invoke "ref_cast_non_null" (i32.const 7)) "null reference")

(assert_return (invoke "ref_cast_null" (i32.const 0)))
(assert_trap (invoke "ref_cast_null" (i32.const 1)) "cast failure")
(assert_trap (invoke "ref_cast_null" (i32.const 2)) "cast failure")
(assert_trap (invoke "ref_cast_null" (i32.const 3)) "cast failure")
(assert_trap (invoke "ref_cast_null" (i32.const 4)) "cast failure")
(assert_return (invoke "ref_cast_null" (i32.const 5)))
(assert_return (invoke "ref_cast_null" (i32.const 6)))
(assert_return (invoke "ref_cast_null" (i32.const 7)))

(assert_trap (invoke "ref_cast_i31" (i32.const 0)) "cast failure")
(assert_return (invoke "ref_cast_i31" (i32.const 1)))
(assert_trap (invoke "ref_cast_i31" (i32.const 2)) "cast failure")
(assert_trap (invoke "ref_cast_i31" (i32.const 3)) "cast failure")
(assert_trap (invoke "ref_cast_i31" (i32.const 4)) "cast failure")
(assert_trap (invoke "ref_cast_i31" (i32.const 5)) "cast failure")
(assert_trap (invoke "ref_cast_i31" (i32.const 6)) "cast failure")
(assert_trap (invoke "ref_cast_i31" (i32.const 7)) "cast failure")

(assert_trap (invoke "ref_cast_struct" (i32.const 0)) "cast failure")
(assert_trap (invoke "ref_cast_struct" (i32.const 1)) "cast failure")
(assert_return (invoke "ref_cast_struct" (i32.const 2)))
(assert_trap (invoke "ref_cast_struct" (i32.const 3)) "cast failure")
(assert_trap (invoke "ref_cast_struct" (i32.const 4)) "cast failure")
(assert_trap (invoke "ref_cast_struct" (i32.const 5)) "cast failure")
(assert_trap (invoke "ref_cast_struct" (i32.const 6)) "cast failure")
(assert_trap (invoke "ref_cast_struct" (i32.const 7)) "cast failure")

(assert_trap (invoke "ref_cast_array" (i32.const 0)) "cast failure")
(assert_trap (invoke "ref_cast_array" (i32.const 1)) "cast failure")
(assert_trap (invoke "ref_cast_array" (i32.const 2)) "cast failure")
(assert_return (invoke "ref_cast_array" (i32.const 3)))
(assert_trap (invoke "ref_cast_array" (i32.const 4)) "cast failure")
(assert_trap (invoke "ref_cast_array" (i32.const 5)) "cast failure")
(assert_trap (invoke "ref_cast_array" (i32.const 6)) "cast failure")
(assert_trap (invoke "ref_cast_array" (i32.const 7)) "cast failure")


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

    (drop (ref.cast (ref null $t0) (ref.null struct)))
    (drop (ref.cast (ref null $t0) (table.get (i32.const 0))))
    (drop (ref.cast (ref null $t0) (table.get (i32.const 1))))
    (drop (ref.cast (ref null $t0) (table.get (i32.const 2))))
    (drop (ref.cast (ref null $t0) (table.get (i32.const 3))))
    (drop (ref.cast (ref null $t0) (table.get (i32.const 4))))

    (drop (ref.cast (ref null $t0) (ref.null struct)))
    (drop (ref.cast (ref null $t1) (table.get (i32.const 1))))
    (drop (ref.cast (ref null $t1) (table.get (i32.const 2))))

    (drop (ref.cast (ref null $t0) (ref.null struct)))
    (drop (ref.cast (ref null $t2) (table.get (i32.const 2))))

    (drop (ref.cast (ref null $t0) (ref.null struct)))
    (drop (ref.cast (ref null $t3) (table.get (i32.const 3))))

    (drop (ref.cast (ref null $t4) (table.get (i32.const 4))))

    (drop (ref.cast (ref $t0) (table.get (i32.const 0))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 1))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 2))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 3))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 4))))

    (drop (ref.cast (ref $t1) (table.get (i32.const 1))))
    (drop (ref.cast (ref $t1) (table.get (i32.const 2))))

    (drop (ref.cast (ref $t2) (table.get (i32.const 2))))

    (drop (ref.cast (ref $t3) (table.get (i32.const 3))))

    (drop (ref.cast (ref $t4) (table.get (i32.const 4))))
  )

  (func (export "test-canon")
    (call $init)

    (drop (ref.cast (ref $t0) (table.get (i32.const 0))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 1))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 2))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 3))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 4))))

    (drop (ref.cast (ref $t0) (table.get (i32.const 10))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 11))))
    (drop (ref.cast (ref $t0) (table.get (i32.const 12))))

    (drop (ref.cast (ref $t1') (table.get (i32.const 1))))
    (drop (ref.cast (ref $t1') (table.get (i32.const 2))))

    (drop (ref.cast (ref $t1) (table.get (i32.const 11))))
    (drop (ref.cast (ref $t1) (table.get (i32.const 12))))

    (drop (ref.cast (ref $t2') (table.get (i32.const 2))))

    (drop (ref.cast (ref $t2) (table.get (i32.const 12))))
  )
)

(invoke "test-sub")
(invoke "test-canon")
