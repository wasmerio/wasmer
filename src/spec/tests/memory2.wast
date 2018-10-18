(module
    (memory (data "a"))
    (func (export "memsize") (result i32)
        (memory.size)
    )
)
