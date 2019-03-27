(module
    (func $main (export "main") (result i32)
        (local $v1 i32)
        (block
            (i32.const 10)
            (set_local $v1)

            (i32.const 42)
            (get_local $v1)
            (i32.add)
            (i32.const 53)
            (i32.eq)
            (br_if 0)

            (i32.const 1)
            (i32.const -100)
            (i32.const 41)
            (i32.lt_s)
            (i32.sub)
            (br_if 0)

            (i32.const -100)
            (i32.const 41)
            (i32.lt_u)
            (br_if 0)

            (i32.const 1)
            (i32.const 100)
            (i32.const -41)
            (i32.gt_s)
            (i32.sub)
            (br_if 0)

            (i32.const 100)
            (i32.const -41)
            (i32.gt_u)
            (br_if 0)

            (i32.const 0)
            (return)
        )
        (unreachable)
    )
)
