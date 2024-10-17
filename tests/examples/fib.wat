(module
    (func $main (result i32)
        (call $fib (i32.const 40))
    )

    (func $fib (param $n i32) (result i32)
        (if (i32.eq (local.get $n) (i32.const 0))
            (then (return (i32.const 1)))
        )
        (if (i32.eq (local.get $n) (i32.const 1))
            (then (return (i32.const 1)))
        )
        (i32.add
            (call $fib (i32.sub (local.get $n) (i32.const 1)))
            (call $fib (i32.sub (local.get $n) (i32.const 2)))
        )
    )

    (export "_start" (func $main))
)
