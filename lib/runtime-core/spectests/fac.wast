(module
  ;; Recursive factorial
  (func (export "fac-rec") (param i64) (result i64)
    (if (result i64) (i64.eq (get_local 0) (i64.const 0))
      (then (i64.const 1))
      (else
        (i64.mul (get_local 0) (call 0 (i64.sub (get_local 0) (i64.const 1))))
      )
    )
  )

  ;; Recursive factorial named
  (func $fac-rec-named (export "fac-rec-named") (param $n i64) (result i64)
    (if (result i64) (i64.eq (get_local $n) (i64.const 0))
      (then (i64.const 1))
      (else
        (i64.mul
          (get_local $n)
          (call $fac-rec-named (i64.sub (get_local $n) (i64.const 1)))
        )
      )
    )
  )

  ;; Iterative factorial
  (func (export "fac-iter") (param i64) (result i64)
    (local i64 i64)
    (set_local 1 (get_local 0))
    (set_local 2 (i64.const 1))
    (block
      (loop
        (if
          (i64.eq (get_local 1) (i64.const 0))
          (then (br 2))
          (else
            (set_local 2 (i64.mul (get_local 1) (get_local 2)))
            (set_local 1 (i64.sub (get_local 1) (i64.const 1)))
          )
        )
        (br 0)
      )
    )
    (get_local 2)
  )

  ;; Iterative factorial named
  (func (export "fac-iter-named") (param $n i64) (result i64)
    (local $i i64)
    (local $res i64)
    (set_local $i (get_local $n))
    (set_local $res (i64.const 1))
    (block $done
      (loop $loop
        (if
          (i64.eq (get_local $i) (i64.const 0))
          (then (br $done))
          (else
            (set_local $res (i64.mul (get_local $i) (get_local $res)))
            (set_local $i (i64.sub (get_local $i) (i64.const 1)))
          )
        )
        (br $loop)
      )
    )
    (get_local $res)
  )

  ;; Optimized factorial.
  (func (export "fac-opt") (param i64) (result i64)
    (local i64)
    (set_local 1 (i64.const 1))
    (block
      (br_if 0 (i64.lt_s (get_local 0) (i64.const 2)))
      (loop
        (set_local 1 (i64.mul (get_local 1) (get_local 0)))
        (set_local 0 (i64.add (get_local 0) (i64.const -1)))
        (br_if 0 (i64.gt_s (get_local 0) (i64.const 1)))
      )
    )
    (get_local 1)
  )
)

(assert_return (invoke "fac-rec" (i64.const 25)) (i64.const 7034535277573963776))
(assert_return (invoke "fac-iter" (i64.const 25)) (i64.const 7034535277573963776))
(assert_return (invoke "fac-rec-named" (i64.const 25)) (i64.const 7034535277573963776))
(assert_return (invoke "fac-iter-named" (i64.const 25)) (i64.const 7034535277573963776))
(assert_return (invoke "fac-opt" (i64.const 25)) (i64.const 7034535277573963776))
(assert_exhaustion (invoke "fac-rec" (i64.const 1073741824)) "call stack exhausted")
