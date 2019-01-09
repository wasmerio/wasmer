;; Test `if` operator

(module
  ;; Auxiliary definition
  (memory 1)

  (func $dummy)

  (func (export "empty") (param i32)
    (if (get_local 0) (then))
    (if (get_local 0) (then) (else))
    (if $l (get_local 0) (then))
    (if $l (get_local 0) (then) (else))
  )

  (func (export "singular") (param i32) (result i32)
    (if (get_local 0) (then (nop)))
    (if (get_local 0) (then (nop)) (else (nop)))
    (if (result i32) (get_local 0) (then (i32.const 7)) (else (i32.const 8)))
  )

  (func (export "multi") (param i32) (result i32)
    (if (get_local 0) (then (call $dummy) (call $dummy) (call $dummy)))
    (if (get_local 0) (then) (else (call $dummy) (call $dummy) (call $dummy)))
    (if (result i32) (get_local 0)
      (then (call $dummy) (call $dummy) (i32.const 8))
      (else (call $dummy) (call $dummy) (i32.const 9))
    )
  )

  (func (export "nested") (param i32 i32) (result i32)
    (if (result i32) (get_local 0)
      (then
        (if (get_local 1) (then (call $dummy) (block) (nop)))
        (if (get_local 1) (then) (else (call $dummy) (block) (nop)))
        (if (result i32) (get_local 1)
          (then (call $dummy) (i32.const 9))
          (else (call $dummy) (i32.const 10))
        )
      )
      (else
        (if (get_local 1) (then (call $dummy) (block) (nop)))
        (if (get_local 1) (then) (else (call $dummy) (block) (nop)))
        (if (result i32) (get_local 1)
          (then (call $dummy) (i32.const 10))
          (else (call $dummy) (i32.const 11))
        )
      )
    )
  )

  (func (export "as-select-first") (param i32) (result i32)
    (select
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 1))
        (else (call $dummy) (i32.const 0))
      )
      (i32.const 2) (i32.const 3)
    )
  )
  (func (export "as-select-mid") (param i32) (result i32)
    (select
      (i32.const 2)
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 1))
        (else (call $dummy) (i32.const 0))
      )
      (i32.const 3)
    )
  )
  (func (export "as-select-last") (param i32) (result i32)
    (select
      (i32.const 2) (i32.const 3)
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 1))
        (else (call $dummy) (i32.const 0))
      )
    )
  )

  (func (export "as-loop-first") (param i32) (result i32)
    (loop (result i32)
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 1))
        (else (call $dummy) (i32.const 0))
      )
      (call $dummy) (call $dummy)
    )
  )
  (func (export "as-loop-mid") (param i32) (result i32)
    (loop (result i32)
      (call $dummy)
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 1))
        (else (call $dummy) (i32.const 0))
      )
      (call $dummy)
    )
  )
  (func (export "as-loop-last") (param i32) (result i32)
    (loop (result i32)
      (call $dummy) (call $dummy)
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 1))
        (else (call $dummy) (i32.const 0))
      )
    )
  )

  (func (export "as-if-condition") (param i32) (result i32)
    (if (result i32)
      (if (result i32) (get_local 0)
        (then (i32.const 1)) (else (i32.const 0))
      )
      (then (call $dummy) (i32.const 2))
      (else (call $dummy) (i32.const 3))
    )
  )

  (func (export "as-br_if-first") (param i32) (result i32)
    (block (result i32)
      (br_if 0
        (if (result i32) (get_local 0)
          (then (call $dummy) (i32.const 1))
          (else (call $dummy) (i32.const 0))
        )
        (i32.const 2)
      )
      (return (i32.const 3))
    )
  )
  (func (export "as-br_if-last") (param i32) (result i32)
    (block (result i32)
      (br_if 0
        (i32.const 2)
        (if (result i32) (get_local 0)
          (then (call $dummy) (i32.const 1))
          (else (call $dummy) (i32.const 0))
        )
      )
      (return (i32.const 3))
    )
  )

  (func (export "as-br_table-first") (param i32) (result i32)
    (block (result i32)
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 1))
        (else (call $dummy) (i32.const 0))
      )
      (i32.const 2)
      (br_table 0 0)
    )
  )
  (func (export "as-br_table-last") (param i32) (result i32)
    (block (result i32)
      (i32.const 2)
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 1))
        (else (call $dummy) (i32.const 0))
      )
      (br_table 0 0)
    )
  )

  (func $func (param i32 i32) (result i32) (get_local 0))
  (type $check (func (param i32 i32) (result i32)))
  (table anyfunc (elem $func))
  (func (export "as-call_indirect-first") (param i32) (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (if (result i32) (get_local 0)
          (then (call $dummy) (i32.const 1))
          (else (call $dummy) (i32.const 0))
        )
        (i32.const 2) (i32.const 0)
      )
    )
  )
  (func (export "as-call_indirect-mid") (param i32) (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (i32.const 2)
        (if (result i32) (get_local 0)
          (then (call $dummy) (i32.const 1))
          (else (call $dummy) (i32.const 0))
        )
        (i32.const 0)
      )
    )
  )
  (func (export "as-call_indirect-last") (param i32) (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (i32.const 2) (i32.const 0)
        (if (result i32) (get_local 0)
          (then (call $dummy) (i32.const 1))
          (else (call $dummy) (i32.const 0))
        )
      )
    )
  )

  (func (export "as-store-first") (param i32)
    (if (result i32) (get_local 0)
      (then (call $dummy) (i32.const 1))
      (else (call $dummy) (i32.const 0))
    )
    (i32.const 2)
    (i32.store)
  )
  (func (export "as-store-last") (param i32)
    (i32.const 2)
    (if (result i32) (get_local 0)
      (then (call $dummy) (i32.const 1))
      (else (call $dummy) (i32.const 0))
    )
    (i32.store)
  )

  (func (export "as-memory.grow-value") (param i32) (result i32)
    (memory.grow
      (if (result i32) (get_local 0)
        (then (i32.const 1))
        (else (i32.const 0))
      )
    )
  )

  (func $f (param i32) (result i32) (get_local 0))

  (func (export "as-call-value") (param i32) (result i32)
    (call $f
      (if (result i32) (get_local 0)
        (then (i32.const 1))
        (else (i32.const 0))
      )
    )
  )
  (func (export "as-return-value") (param i32) (result i32)
    (if (result i32) (get_local 0)
      (then (i32.const 1))
      (else (i32.const 0)))
    (return)
  )
  (func (export "as-drop-operand") (param i32)
    (drop
      (if (result i32) (get_local 0)
        (then (i32.const 1))
        (else (i32.const 0))
      )
    )
  )
  (func (export "as-br-value") (param i32) (result i32)
    (block (result i32)
      (br 0
        (if (result i32) (get_local 0)
          (then (i32.const 1))
          (else (i32.const 0))
        )
      )
    )
  )
  (func (export "as-set_local-value") (param i32) (result i32)
    (local i32)
    (set_local 0
      (if (result i32) (get_local 0)
        (then (i32.const 1))
        (else (i32.const 0))
      )
    )
    (get_local 0)
  )
  (func (export "as-tee_local-value") (param i32) (result i32)
    (tee_local 0
      (if (result i32) (get_local 0)
        (then (i32.const 1))
        (else (i32.const 0))
      )
    )
  )
  (global $a (mut i32) (i32.const 10))
  (func (export "as-set_global-value") (param i32) (result i32)
    (set_global $a
      (if (result i32) (get_local 0)
        (then (i32.const 1))
        (else (i32.const 0))
      )
    ) (get_global $a)
  )
  (func (export "as-load-operand") (param i32) (result i32)
    (i32.load
      (if (result i32) (get_local 0)
        (then (i32.const 11))
        (else (i32.const 10))
      )
    )
  )

  (func (export "as-unary-operand") (param i32) (result i32)
    (i32.ctz
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 13))
        (else (call $dummy) (i32.const -13))
      )
    )
  )
  (func (export "as-binary-operand") (param i32 i32) (result i32)
    (i32.mul
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 3))
        (else (call $dummy) (i32.const -3))
      )
      (if (result i32) (get_local 1)
        (then (call $dummy) (i32.const 4))
        (else (call $dummy) (i32.const -5))
      )
    )
  )
  (func (export "as-test-operand") (param i32) (result i32)
    (i32.eqz
      (if (result i32) (get_local 0)
        (then (call $dummy) (i32.const 13))
        (else (call $dummy) (i32.const 0))
      )
    )
  )
  (func (export "as-compare-operand") (param i32 i32) (result i32)
    (f32.gt
      (if (result f32) (get_local 0)
        (then (call $dummy) (f32.const 3))
        (else (call $dummy) (f32.const -3))
      )
      (if (result f32) (get_local 1)
        (then (call $dummy) (f32.const 4))
        (else (call $dummy) (f32.const -4))
      )
    )
  )

  (func (export "break-bare") (result i32)
    (if (i32.const 1) (then (br 0) (unreachable)))
    (if (i32.const 1) (then (br 0) (unreachable)) (else (unreachable)))
    (if (i32.const 0) (then (unreachable)) (else (br 0) (unreachable)))
    (if (i32.const 1) (then (br_if 0 (i32.const 1)) (unreachable)))
    (if (i32.const 1) (then (br_if 0 (i32.const 1)) (unreachable)) (else (unreachable)))
    (if (i32.const 0) (then (unreachable)) (else (br_if 0 (i32.const 1)) (unreachable)))
    (if (i32.const 1) (then (br_table 0 (i32.const 0)) (unreachable)))
    (if (i32.const 1) (then (br_table 0 (i32.const 0)) (unreachable)) (else (unreachable)))
    (if (i32.const 0) (then (unreachable)) (else (br_table 0 (i32.const 0)) (unreachable)))
    (i32.const 19)
  )

  (func (export "break-value") (param i32) (result i32)
    (if (result i32) (get_local 0)
      (then (br 0 (i32.const 18)) (i32.const 19))
      (else (br 0 (i32.const 21)) (i32.const 20))
    )
  )

  (func (export "effects") (param i32) (result i32)
    (local i32)
    (if
      (block (result i32) (set_local 1 (i32.const 1)) (get_local 0))
      (then
        (set_local 1 (i32.mul (get_local 1) (i32.const 3)))
        (set_local 1 (i32.sub (get_local 1) (i32.const 5)))
        (set_local 1 (i32.mul (get_local 1) (i32.const 7)))
        (br 0)
        (set_local 1 (i32.mul (get_local 1) (i32.const 100)))
      )
      (else
        (set_local 1 (i32.mul (get_local 1) (i32.const 5)))
        (set_local 1 (i32.sub (get_local 1) (i32.const 7)))
        (set_local 1 (i32.mul (get_local 1) (i32.const 3)))
        (br 0)
        (set_local 1 (i32.mul (get_local 1) (i32.const 1000)))
      )
    )
    (get_local 1)
  )
)

(assert_return (invoke "empty" (i32.const 0)))
(assert_return (invoke "empty" (i32.const 1)))
(assert_return (invoke "empty" (i32.const 100)))
(assert_return (invoke "empty" (i32.const -2)))

(assert_return (invoke "singular" (i32.const 0)) (i32.const 8))
(assert_return (invoke "singular" (i32.const 1)) (i32.const 7))
(assert_return (invoke "singular" (i32.const 10)) (i32.const 7))
(assert_return (invoke "singular" (i32.const -10)) (i32.const 7))

(assert_return (invoke "multi" (i32.const 0)) (i32.const 9))
(assert_return (invoke "multi" (i32.const 1)) (i32.const 8))
(assert_return (invoke "multi" (i32.const 13)) (i32.const 8))
(assert_return (invoke "multi" (i32.const -5)) (i32.const 8))

(assert_return (invoke "nested" (i32.const 0) (i32.const 0)) (i32.const 11))
(assert_return (invoke "nested" (i32.const 1) (i32.const 0)) (i32.const 10))
(assert_return (invoke "nested" (i32.const 0) (i32.const 1)) (i32.const 10))
(assert_return (invoke "nested" (i32.const 3) (i32.const 2)) (i32.const 9))
(assert_return (invoke "nested" (i32.const 0) (i32.const -100)) (i32.const 10))
(assert_return (invoke "nested" (i32.const 10) (i32.const 10)) (i32.const 9))
(assert_return (invoke "nested" (i32.const 0) (i32.const -1)) (i32.const 10))
(assert_return (invoke "nested" (i32.const -111) (i32.const -2)) (i32.const 9))

(assert_return (invoke "as-select-first" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-select-first" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-select-mid" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-select-mid" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-select-last" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-select-last" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-loop-first" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-loop-first" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-loop-mid" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-loop-mid" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-loop-last" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-loop-last" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-if-condition" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-if-condition" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-br_if-first" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-br_if-first" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-br_if-last" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-br_if-last" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-br_table-first" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-br_table-first" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-br_table-last" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-br_table-last" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-call_indirect-first" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-call_indirect-first" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-call_indirect-mid" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-call_indirect-mid" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-call_indirect-last" (i32.const 0)) (i32.const 2))
(assert_trap (invoke "as-call_indirect-last" (i32.const 1)) "undefined element")

(assert_return (invoke "as-store-first" (i32.const 0)))
(assert_return (invoke "as-store-first" (i32.const 1)))
(assert_return (invoke "as-store-last" (i32.const 0)))
(assert_return (invoke "as-store-last" (i32.const 1)))

(assert_return (invoke "as-memory.grow-value" (i32.const 0)) (i32.const 1))
(assert_return (invoke "as-memory.grow-value" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-call-value" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-call-value" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-return-value" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-return-value" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-drop-operand" (i32.const 0)))
(assert_return (invoke "as-drop-operand" (i32.const 1)))

(assert_return (invoke "as-br-value" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-br-value" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-set_local-value" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-set_local-value" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-tee_local-value" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-tee_local-value" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-set_global-value" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-set_global-value" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-load-operand" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-load-operand" (i32.const 1)) (i32.const 0))

(assert_return (invoke "as-unary-operand" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-unary-operand" (i32.const 1)) (i32.const 0))
(assert_return (invoke "as-unary-operand" (i32.const -1)) (i32.const 0))

(assert_return (invoke "as-binary-operand" (i32.const 0) (i32.const 0)) (i32.const 15))
(assert_return (invoke "as-binary-operand" (i32.const 0) (i32.const 1)) (i32.const -12))
(assert_return (invoke "as-binary-operand" (i32.const 1) (i32.const 0)) (i32.const -15))
(assert_return (invoke "as-binary-operand" (i32.const 1) (i32.const 1)) (i32.const 12))

(assert_return (invoke "as-test-operand" (i32.const 0)) (i32.const 1))
(assert_return (invoke "as-test-operand" (i32.const 1)) (i32.const 0))

(assert_return (invoke "as-compare-operand" (i32.const 0) (i32.const 0)) (i32.const 1))
(assert_return (invoke "as-compare-operand" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "as-compare-operand" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "as-compare-operand" (i32.const 1) (i32.const 1)) (i32.const 0))

(assert_return (invoke "break-bare") (i32.const 19))
(assert_return (invoke "break-value" (i32.const 1)) (i32.const 18))
(assert_return (invoke "break-value" (i32.const 0)) (i32.const 21))

(assert_return (invoke "effects" (i32.const 1)) (i32.const -14))
(assert_return (invoke "effects" (i32.const 0)) (i32.const -6))

(assert_invalid
  (module (func $type-empty-i32 (result i32) (if (i32.const 0) (then))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-i64 (result i64) (if (i32.const 0) (then))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f32 (result f32) (if (i32.const 0) (then))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f64 (result f64) (if (i32.const 0) (then))))
  "type mismatch"
)

(assert_invalid
  (module (func $type-empty-i32 (result i32) (if (i32.const 0) (then) (else))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-i64 (result i64) (if (i32.const 0) (then) (else))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f32 (result f32) (if (i32.const 0) (then) (else))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f64 (result f64) (if (i32.const 0) (then) (else))))
  "type mismatch"
)

(assert_invalid
  (module (func $type-then-value-num-vs-void
    (if (i32.const 1) (then (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-then-value-num-vs-void
    (if (i32.const 1) (then (i32.const 1)) (else))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-else-value-num-vs-void
    (if (i32.const 1) (then) (else (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-both-value-num-vs-void
    (if (i32.const 1) (then (i32.const 1)) (else (i32.const 1)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-then-value-empty-vs-num (result i32)
    (if (result i32) (i32.const 1) (then) (else (i32.const 0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-then-value-empty-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (i32.const 0)) (else))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-both-value-empty-vs-num (result i32)
    (if (result i32) (i32.const 1) (then) (else))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-no-else-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (i32.const 1)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-then-value-void-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (nop)) (else (i32.const 0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-then-value-void-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (i32.const 0)) (else (nop)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-both-value-void-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (nop)) (else (nop)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-then-value-num-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (i64.const 1)) (else (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-then-value-num-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (i32.const 1)) (else (i64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-both-value-num-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (i64.const 1)) (else (i64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-both-different-value-num-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (i64.const 1)) (else (f64.const 1)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-then-value-unreached-select (result i32)
    (if (result i64)
      (i32.const 0)
      (then (select (unreachable) (unreachable) (unreachable)))
      (else (i64.const 0))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-else-value-unreached-select (result i32)
    (if (result i64)
      (i32.const 1)
      (then (i64.const 0))
      (else (select (unreachable) (unreachable) (unreachable)))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-else-value-unreached-select (result i32)
    (if (result i64)
      (i32.const 1)
      (then (select (unreachable) (unreachable) (unreachable)))
      (else (select (unreachable) (unreachable) (unreachable)))
    )
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-then-break-last-void-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (br 0)) (else (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-else-break-last-void-vs-num (result i32)
    (if (result i32) (i32.const 1) (then (i32.const 1)) (else (br 0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-then-break-empty-vs-num (result i32)
    (if (result i32) (i32.const 1)
      (then (br 0) (i32.const 1))
      (else (i32.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-else-break-empty-vs-num (result i32)
    (if (result i32) (i32.const 1)
      (then (i32.const 1))
      (else (br 0) (i32.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-then-break-void-vs-num (result i32)
    (if (result i32) (i32.const 1)
      (then (br 0 (nop)) (i32.const 1))
      (else (i32.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-else-break-void-vs-num (result i32)
    (if (result i32) (i32.const 1)
      (then (i32.const 1))
      (else (br 0 (nop)) (i32.const 1))
    )
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-then-break-num-vs-num (result i32)
    (if (result i32) (i32.const 1)
      (then (br 0 (i64.const 1)) (i32.const 1))
      (else (i32.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-else-break-num-vs-num (result i32)
    (if (result i32) (i32.const 1)
      (then (i32.const 1))
      (else (br 0 (i64.const 1)) (i32.const 1))
    )
  ))
  "type mismatch"
)


(assert_malformed
  (module quote "(func if end $l)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if $a end $l)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if else $l end)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if $a else $l end)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if else end $l)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if else $l end $l)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if else $l1 end $l2)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if $a else end $l)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if $a else $a end $l)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func if $a else $l end $l)")
  "mismatching label"
)
