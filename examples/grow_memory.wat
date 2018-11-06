(module
    (memory 1)
    (func $main (export "main") (result i32)
        (drop (memory.grow (i32.const 1)))
        (i32.store (i32.const 400) (i32.const 1600))
        (i32.load (i32.const 1600))
    )
)
