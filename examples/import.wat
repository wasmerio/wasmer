(module
    (memory (import "env" "memory") 1)
    (table (import "env" "table") 10 anyfunc)
    (elem (i32.const 9) $f)
    (func $f (param i32) (result i32)
        (get_local 0)
    )
    (func $main (export "main") (result i32)
        (local i32)
        (set_local 0 (i32.const 65535))
        (i32.store (get_local 0) (i32.const 1602))
        (i32.load (get_local 0))

        (drop)

        (call_indirect (param i32) (result i32) (i32.const 4505) (i32.const 9))

        (drop)

        (memory.grow (i32.const 1))

        (drop)

        (set_local 0 (i32.const 131071))
        (i32.store (get_local 0) (i32.const 1455))
        (i32.load (get_local 0))
    )
)
