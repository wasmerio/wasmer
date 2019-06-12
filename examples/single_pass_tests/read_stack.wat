(module
    (type $t1 (func))
	(func $stack_read (import "wasi_unstable" "stack_read") (type $t1))

    (func $_start (export "_start")
        (if (i32.ne (call $fib (i32.const 1)) (i32.const 1))
            (then unreachable)
        )
    )

    (func $fib (param $x i32) (result i32)
        (call $stack_read)
        (if (result i32) (i32.or (i32.eq (get_local $x) (i32.const 1)) (i32.eq (get_local $x) (i32.const 2)))
            (then (i32.const 1))
            (else (i32.add
                (call $fib (i32.sub (get_local $x) (i32.const 1)))
                (call $fib (i32.sub (get_local $x) (i32.const 2)))
            ))
        )
    )
)
