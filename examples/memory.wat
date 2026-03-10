(module
    (memory 1)
    (table 20 anyfunc)
    (elem (i32.const 9) $f)
    (func $f (param i32) (result i32)
        (get_local 0)
    )
    (func $main (export "main") (result i32)
        (local i32)
        (set_local 0 (i32.const 100))
        (i32.store (get_local 0) (i32.const 1602))
        (i32.load (get_local 0))

        (drop)
        (memory.grow (i32.const 0))

        (drop)
        (memory.grow (i32.const 2))

        (drop)
        (memory.grow (i32.const 65536))
        
        (drop)
        (memory.grow (i32.const 12))
    )
)
