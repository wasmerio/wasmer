(module
    (memory 1)
    (func $grow_mem (result i32)
        (grow_memory (i32.const 1))
    )
    (func $main (export "main") (result i32)
        (call $grow_mem)
    )
)
