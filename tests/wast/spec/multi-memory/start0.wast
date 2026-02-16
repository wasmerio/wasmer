(module
  (memory 0)
  (memory $m (data "A"))
  (memory $n 1)
  
  (func $inc
    (i32.store8 $m
      (i32.const 0)
      (i32.add
        (i32.load8_u $m (i32.const 0))
        (i32.const 1)
      )
    )
  )
  (func $get (result i32)
    (return (i32.load8_u $m (i32.const 0)))
  )
  (func $getn (result i32)
    (return (i32.load8_u $n (i32.const 0)))
  )
  (func $main
    (call $inc)
    (call $inc)
    (call $inc)
  )

  (start $main)
  (export "inc" (func $inc))
  (export "get" (func $get))
  (export "getn" (func $getn))
)
(assert_return (invoke "get") (i32.const 68))
(assert_return (invoke "getn") (i32.const 0))

(invoke "inc")
(assert_return (invoke "get") (i32.const 69))
(assert_return (invoke "getn") (i32.const 0))

(invoke "inc")
(assert_return (invoke "get") (i32.const 70))
(assert_return (invoke "getn") (i32.const 0))

