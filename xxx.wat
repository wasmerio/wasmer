(module
    (func $dot_product_example
            (param $x0 f64) (param $x1 f64) (param $x2 f64) (param $x3 f64)
            (param $y0 f64) (param $y1 f64) (param $y2 f64) (param $y3 f64)
            (result f64)
        (f64.add (f64.add (f64.add
        (f64.mul (local.get $x0) (local.get $y0))
        (f64.mul (local.get $x1) (local.get $y1)))
        (f64.mul (local.get $x2) (local.get $y2)))
        (f64.mul (local.get $x3) (local.get $y3)))
    )
    (func $main (export "main")
        (param i32) (param i32) (param i32) (param i32)
        (param i32) (param i32) (param i32) (param i32)
        (result i32)
        (i32.add
            (get_local 0)
            (i32.add
                (get_local 1)
                (i32.add
                    (get_local 2)
                    (i32.add
                        (get_local 3)
                        (i32.add
                            (get_local 4)
                            (i32.add
                                (get_local 5)
                                (i32.add
                                    (get_local 6)
                                    (get_local 7)
                                )
                            )
                        )
                    )
                )
            )
        )
    )
)