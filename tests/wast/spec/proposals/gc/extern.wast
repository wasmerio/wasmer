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
  )

  (func (export "internalize") (param externref) (result anyref)
    (any.convert_extern (local.get 0))
  )
  (func (export "externalize") (param anyref) (result externref)
    (extern.convert_any (local.get 0))
  )

  (func (export "externalize-i") (param i32) (result externref)
    (extern.convert_any (table.get (local.get 0)))
  )
  (func (export "externalize-ii") (param i32) (result anyref)
    (any.convert_extern (extern.convert_any (table.get (local.get 0))))
  )
)

(invoke "init" (ref.extern 0))

(assert_return (invoke "internalize" (ref.extern 1)) (ref.host 1))
(assert_return (invoke "internalize" (ref.null extern)) (ref.null any))

(assert_return (invoke "externalize" (ref.host 2)) (ref.extern 2))
(assert_return (invoke "externalize" (ref.null any)) (ref.null extern))

(assert_return (invoke "externalize-i" (i32.const 0)) (ref.null extern))
(assert_return (invoke "externalize-i" (i32.const 1)) (ref.extern))
(assert_return (invoke "externalize-i" (i32.const 2)) (ref.extern))
(assert_return (invoke "externalize-i" (i32.const 3)) (ref.extern))
(assert_return (invoke "externalize-i" (i32.const 4)) (ref.extern))
(assert_return (invoke "externalize-i" (i32.const 5)) (ref.null extern))

(assert_return (invoke "externalize-ii" (i32.const 0)) (ref.null any))
(assert_return (invoke "externalize-ii" (i32.const 1)) (ref.i31))
(assert_return (invoke "externalize-ii" (i32.const 2)) (ref.struct))
(assert_return (invoke "externalize-ii" (i32.const 3)) (ref.array))
(assert_return (invoke "externalize-ii" (i32.const 4)) (ref.host 0))
(assert_return (invoke "externalize-ii" (i32.const 5)) (ref.null any))
