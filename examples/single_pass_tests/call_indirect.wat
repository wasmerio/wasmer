(module
    (type $binop (func (param i32 i32) (result i32)))
    (table 1 100 anyfunc)
    (elem (i32.const 10) $add)

    (func $main (export "main") (result i32)
        (call_indirect (type $binop) (i32.const 42) (i32.const 1) (i32.const 9))
    )
    (func $add (param i32) (param i32) (result i32)
        (i32.add (get_local 0) (get_local 1))
    )
)
