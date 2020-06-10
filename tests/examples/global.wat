(module
    (global $xxx (mut i32) (i32.const 42))
    (func $main (result i32)
        (global.set $xxx (i32.const 0))
        (i32.const 1)
    )
    (export "_start" (func $main))
)
