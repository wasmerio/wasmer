(assert_invalid
  (module (func) (start 1))
  "unknown function"
)

(assert_invalid
  (module
    (func $main (result i32) (return (i32.const 0)))
    (start $main)
  )
  "start function"
)
(assert_invalid
  (module
    (func $main (param $a i32))
    (start $main)
  )
  "start function"
)

(module
  (memory (data "A"))
  (func $inc
    (i32.store8
      (i32.const 0)
      (i32.add
        (i32.load8_u (i32.const 0))
        (i32.const 1)
      )
    )
  )
  (func $get (result i32)
    (return (i32.load8_u (i32.const 0)))
  )
  (func $main
    (call $inc)
    (call $inc)
    (call $inc)
  )

  (start $main)
  (export "inc" (func $inc))
  (export "get" (func $get))
)
(assert_return (invoke "get") (i32.const 68))
(invoke "inc")
(assert_return (invoke "get") (i32.const 69))
(invoke "inc")
(assert_return (invoke "get") (i32.const 70))

(module
  (memory (data "A"))
  (func $inc
    (i32.store8
      (i32.const 0)
      (i32.add
        (i32.load8_u (i32.const 0))
        (i32.const 1)
      )
    )
  )
  (func $get (result i32)
    (return (i32.load8_u (i32.const 0)))
  )
  (func $main
    (call $inc)
    (call $inc)
    (call $inc)
  )
  (start 2)
  (export "inc" (func $inc))
  (export "get" (func $get))
)
(assert_return (invoke "get") (i32.const 68))
(invoke "inc")
(assert_return (invoke "get") (i32.const 69))
(invoke "inc")
(assert_return (invoke "get") (i32.const 70))

(module
  (func $print_i32 (import "spectest" "print_i32") (param i32))
  (func $main (call $print_i32 (i32.const 1)))
  (start 1)
)

(module
  (func $print_i32 (import "spectest" "print_i32") (param i32))
  (func $main (call $print_i32 (i32.const 2)))
  (start $main)
)

(module
  (func $print (import "spectest" "print"))
  (start $print)
)

(assert_trap
  (module (func $main (unreachable)) (start $main))
  "unreachable"
)

(assert_malformed
  (module quote "(module (func $a (unreachable)) (func $b (unreachable)) (start $a) (start $b))")
  "multiple start sections"
)
