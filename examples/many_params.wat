;; Test case for correctness of reading state with the presence of parameters passed on (machine) stack.
;; Usage: Run with a backend with support for OSR. Interrupt execution randomly.
;; Should see the stack frame for `$foo` to have locals `[0] = 1, [1] = 2, [2] = 3, [3] = 4, [4] = 5, [5] = 6, [6] = 7, [7] = 8` with high probability.
;; If the logic for reading stack parameters is broken, it's likely to see `[0] = 1, [1] = 2, [2] = 3, [3] = 4, [4] = 5, [5] = ?, [6] = ?, [7] = ?`.

(module
    (import "wasi_unstable" "proc_exit" (func $__wasi_proc_exit (param i32)))
    (func $long_running
        (local $count i32)
        (loop
            (if (i32.eq (get_local $count) (i32.const 1000000)) (then (return)))
            (set_local $count (i32.add (i32.const 1) (get_local $count)))
            (br 0)
        )
        (unreachable)
    )

    (func $foo (param i32) (param i64) (param i32) (param i32) (param i32) (param i64) (param i64) (param i64) (result i32)
        (set_local 2 (i32.const 3))
        (call $long_running)
        (i32.add
            (i32.mul (i32.const 2) (get_local 0))
            (i32.add
                (i32.mul (i32.const 3) (i32.wrap/i64 (get_local 1)))
                (i32.add
                    (i32.mul (i32.const 5) (get_local 2))
                    (i32.add
                        (i32.mul (i32.const 7) (get_local 3))
                        (i32.add
                            (i32.mul (i32.const 11) (get_local 4))
                            (i32.add
                                (i32.mul (i32.const 13) (i32.wrap/i64 (get_local 5)))
                                (i32.add
                                    (i32.mul (i32.const 17) (i32.wrap/i64 (get_local 6)))
                                    (i32.mul (i32.const 19) (i32.wrap/i64 (get_local 7)))
                                )
                            )
                        )
                    )
                )
            )
        )
    )
    (func $_start (export "_start")
        (local $count i32)
        (loop
            (if (i32.eq (get_local $count) (i32.const 10000)) (then (return)))
            (set_local $count (i32.add (i32.const 1) (get_local $count)))
            (call $foo (i32.const 1) (i64.const 2) (i32.const 30) (i32.const 4) (i32.const 5) (i64.const 6) (i64.const 7) (i64.const 8))
            (if (i32.ne (i32.const 455))
                (then unreachable)
            )
            (br 0)
        )
        (unreachable)
    )
)
