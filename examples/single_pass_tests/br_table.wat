(module
    (func $main (export "main")
        (i32.eq (call $test (i32.const 0)) (i32.const 2))
        (i32.eq (call $test (i32.const 1)) (i32.const 0))
        (i32.eq (call $test (i32.const 2)) (i32.const 1))
        (i32.eq (call $test (i32.const 3)) (i32.const 3))
        (i32.eq (call $test (i32.const 4)) (i32.const 3))
        (i32.and)
        (i32.and)
        (i32.and)
        (i32.and)
        (i32.const 1)
        (i32.eq)
        (br_if 0)
        (unreachable)
    )

    (func $test (param $p i32) (result i32)
        (block
            (block
                (block
                    (block
                        (block
                            (get_local $p)
                            (br_table 2 0 1 3)
                        )
                        (return (i32.const 0))
                    )
                    (return (i32.const 1))
                )
                (return (i32.const 2))
            )
            (return (i32.const 3))
        )
        (unreachable)
    )
)
