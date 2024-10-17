;; Abstract Types

(module
  (type $ft (func (result i32)))
  (type $st (struct (field i16)))
  (type $at (array i8))

  (table 10 anyref)

  (elem declare func $f)
  (func $f (result i32) (i32.const 9))

  (func (export "init") (param $x externref)
    (table.set (i32.const 0) (ref.null any))
    (table.set (i32.const 1) (ref.i31 (i32.const 7)))
    (table.set (i32.const 2) (struct.new $st (i32.const 6)))
    (table.set (i32.const 3) (array.new $at (i32.const 5) (i32.const 3)))
    (table.set (i32.const 4) (any.convert_extern (local.get $x)))
  )

  (func (export "br_on_null") (param $i i32) (result i32)
    (block $l
      (br_on_null $l (table.get (local.get $i)))
      (return (i32.const -1))
    )
    (i32.const 0)
  )
  (func (export "br_on_i31") (param $i i32) (result i32)
    (block $l (result (ref i31))
      (br_on_cast $l anyref (ref i31) (table.get (local.get $i)))
      (return (i32.const -1))
    )
    (i31.get_u)
  )
  (func (export "br_on_struct") (param $i i32) (result i32)
    (block $l (result (ref struct))
      (br_on_cast $l anyref (ref struct) (table.get (local.get $i)))
      (return (i32.const -1))
    )
    (block $l2 (param structref) (result (ref $st))
      (block $l3 (param structref) (result (ref $at))
        (br_on_cast $l2 structref (ref $st))
        (br_on_cast $l3 anyref (ref $at))
        (return (i32.const -2))
      )
      (return (array.get_u $at (i32.const 0)))
    )
    (struct.get_s $st 0)
  )
  (func (export "br_on_array") (param $i i32) (result i32)
    (block $l (result (ref array))
      (br_on_cast $l anyref (ref array) (table.get (local.get $i)))
      (return (i32.const -1))
    )
    (array.len)
  )

  (func (export "null-diff") (param $i i32) (result i32)
    (block $l (result (ref null struct))
      (block (result (ref any))
        (br_on_cast $l (ref null any) (ref null struct) (table.get (local.get $i)))
      )
      (return (i32.const 0))
    )
    (return (i32.const 1))
  )
)

(invoke "init" (ref.extern 0))

(assert_return (invoke "br_on_null" (i32.const 0)) (i32.const 0))
(assert_return (invoke "br_on_null" (i32.const 1)) (i32.const -1))
(assert_return (invoke "br_on_null" (i32.const 2)) (i32.const -1))
(assert_return (invoke "br_on_null" (i32.const 3)) (i32.const -1))
(assert_return (invoke "br_on_null" (i32.const 4)) (i32.const -1))

(assert_return (invoke "br_on_i31" (i32.const 0)) (i32.const -1))
(assert_return (invoke "br_on_i31" (i32.const 1)) (i32.const 7))
(assert_return (invoke "br_on_i31" (i32.const 2)) (i32.const -1))
(assert_return (invoke "br_on_i31" (i32.const 3)) (i32.const -1))
(assert_return (invoke "br_on_i31" (i32.const 4)) (i32.const -1))

(assert_return (invoke "br_on_struct" (i32.const 0)) (i32.const -1))
(assert_return (invoke "br_on_struct" (i32.const 1)) (i32.const -1))
(assert_return (invoke "br_on_struct" (i32.const 2)) (i32.const 6))
(assert_return (invoke "br_on_struct" (i32.const 3)) (i32.const -1))
(assert_return (invoke "br_on_struct" (i32.const 4)) (i32.const -1))

(assert_return (invoke "br_on_array" (i32.const 0)) (i32.const -1))
(assert_return (invoke "br_on_array" (i32.const 1)) (i32.const -1))
(assert_return (invoke "br_on_array" (i32.const 2)) (i32.const -1))
(assert_return (invoke "br_on_array" (i32.const 3)) (i32.const 3))
(assert_return (invoke "br_on_array" (i32.const 4)) (i32.const -1))

(assert_return (invoke "null-diff" (i32.const 0)) (i32.const 1))
(assert_return (invoke "null-diff" (i32.const 1)) (i32.const 0))
(assert_return (invoke "null-diff" (i32.const 2)) (i32.const 1))
(assert_return (invoke "null-diff" (i32.const 3)) (i32.const 0))
(assert_return (invoke "null-diff" (i32.const 4)) (i32.const 0))


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

  (table 20 structref)

  (func $init
    (table.set (i32.const 0) (struct.new_default $t0))
    (table.set (i32.const 10) (struct.new_default $t0'))
    (table.set (i32.const 1) (struct.new_default $t1))
    (table.set (i32.const 11) (struct.new_default $t1'))
    (table.set (i32.const 2) (struct.new_default $t2))
    (table.set (i32.const 12) (struct.new_default $t2'))
    (table.set (i32.const 3) (struct.new_default $t3))
    (table.set (i32.const 4) (struct.new_default $t4))
  )

  (func (export "test-sub")
    (call $init)
    (block $l (result structref)
      ;; must succeed
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (ref.null struct))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (table.get (i32.const 0)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (table.get (i32.const 1)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (table.get (i32.const 2)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (table.get (i32.const 3)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (table.get (i32.const 4)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t1) (ref.null struct))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t1) (table.get (i32.const 1)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t1) (table.get (i32.const 2)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t2) (ref.null struct))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t2) (table.get (i32.const 2)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t3) (ref.null struct))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t3) (table.get (i32.const 3)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t4) (ref.null struct))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t4) (table.get (i32.const 4)))))

      ;; must not succeed
      (br_on_cast $l anyref (ref $t1) (table.get (i32.const 0)))
      (br_on_cast $l anyref (ref $t1) (table.get (i32.const 3)))
      (br_on_cast $l anyref (ref $t1) (table.get (i32.const 4)))

      (br_on_cast $l anyref (ref $t2) (table.get (i32.const 0)))
      (br_on_cast $l anyref (ref $t2) (table.get (i32.const 1)))
      (br_on_cast $l anyref (ref $t2) (table.get (i32.const 3)))
      (br_on_cast $l anyref (ref $t2) (table.get (i32.const 4)))

      (br_on_cast $l anyref (ref $t3) (table.get (i32.const 0)))
      (br_on_cast $l anyref (ref $t3) (table.get (i32.const 1)))
      (br_on_cast $l anyref (ref $t3) (table.get (i32.const 2)))
      (br_on_cast $l anyref (ref $t3) (table.get (i32.const 4)))

      (br_on_cast $l anyref (ref $t4) (table.get (i32.const 0)))
      (br_on_cast $l anyref (ref $t4) (table.get (i32.const 1)))
      (br_on_cast $l anyref (ref $t4) (table.get (i32.const 2)))
      (br_on_cast $l anyref (ref $t4) (table.get (i32.const 3)))

      (return)
    )
    (unreachable)
  )

  (func (export "test-canon")
    (call $init)
    (block $l
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0') (table.get (i32.const 0)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0') (table.get (i32.const 1)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0') (table.get (i32.const 2)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0') (table.get (i32.const 3)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0') (table.get (i32.const 4)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (table.get (i32.const 10)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (table.get (i32.const 11)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t0) (table.get (i32.const 12)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t1') (table.get (i32.const 1)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t1') (table.get (i32.const 2)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t1) (table.get (i32.const 11)))))
      (drop (block (result structref) (br_on_cast 0 structref (ref $t1) (table.get (i32.const 12)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t2') (table.get (i32.const 2)))))

      (drop (block (result structref) (br_on_cast 0 structref (ref $t2) (table.get (i32.const 12)))))

      (return)
    )
    (unreachable)
  )
)

(invoke "test-sub")
(invoke "test-canon")


;; Cases of nullability

(module
  (type $t (struct))

  (func (param (ref any)) (result (ref $t))
    (block (result (ref any)) (br_on_cast 1 (ref any) (ref $t) (local.get 0))) (unreachable)
  )
  (func (param (ref null any)) (result (ref $t))
    (block (result (ref null any)) (br_on_cast 1 (ref null any) (ref $t) (local.get 0))) (unreachable)
  )
  (func (param (ref null any)) (result (ref null $t))
    (block (result (ref null any)) (br_on_cast 1 (ref null any) (ref null $t) (local.get 0))) (unreachable)
  )
)

(assert_invalid
  (module
    (type $t (struct))
    (func (param (ref any)) (result (ref $t))
      (block (result (ref any)) (br_on_cast 1 (ref null any) (ref null $t) (local.get 0))) (unreachable)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type $t (struct))
    (func (param (ref any)) (result (ref null $t))
      (block (result (ref any)) (br_on_cast 1 (ref any) (ref null $t) (local.get 0))) (unreachable)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type $t (struct))
    (func (param (ref null any)) (result (ref $t))
      (block (result (ref any)) (br_on_cast 1 (ref null any) (ref $t) (local.get 0))) (unreachable)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func (result anyref)
      (br_on_cast 0 eqref anyref (unreachable))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func (result anyref)
      (br_on_cast 0 structref arrayref (unreachable))
    )
  )
  "type mismatch"
)


;; https://github.com/WebAssembly/gc/issues/516
(assert_invalid
  (module
    (type $t (func))
    (func $f (param (ref null $t)) (result funcref) (local.get 0))
    (func (param funcref) (result funcref funcref)
      (ref.null $t)
      (local.get 0)
      (br_on_cast 0 funcref (ref $t))  ;; only leaves two funcref's on the stack
      (drop)
      (call $f)
      (local.get 0)
    )
  )
  "type mismatch"
)
