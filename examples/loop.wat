(module
    ;; dummy memory
    (memory 1)

    ;; Entry point
    (func $main (result i32)
        (local $total i32)
        (local $count i32)
        (set_local $count (i32.const 10)) ;; Giving $count an inital value of 10

        ;; Iteratively decrement $count and increment $total by 2
        (loop $loop
            (if (i32.eqz (get_local $count))
                (then)
                (else
                    (set_local $count (i32.sub (get_local $count) (i32.const 1)))
                    (set_local $total (i32.add (get_local $total) (i32.const 2)))
                    (br $loop)
                )
            )
        )
        (get_local $total)
    )

    (export "main" (func $main))
)
