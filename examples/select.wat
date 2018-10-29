(module
    (func $ternary (param $lhs i64) (param $rhs i64) (param $cond i32) (result i64)
        (select
            (get_local $lhs)
            (get_local $rhs)
            (get_local $cond)
        )
    )

    (func $main (result i64)
        (call $ternary
            (i64.const 126)
            (call $ternary
                (i64.const 1024)
                (i64.const 4028)
                (i32.const 0)
            )
            (i32.const 0)
        )
    )

    (export "main" (func $main))
)
