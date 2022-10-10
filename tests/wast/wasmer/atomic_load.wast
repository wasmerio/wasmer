(module
    (memory 1)
    (func (export "atomic_load")
        i32.const 0xffff_fff0
        i32.atomic.load offset=16
        drop
    )
)
(assert_trap (invoke "atomic_load") "out of bound")
