;; Test `br` operator

(module
  ;; Auxiliary definition
  (func $dummy)

  (func (export "type-i32") (block (drop (i32.ctz (br 0)))))
  (func (export "type-i64") (block (drop (i64.ctz (br 0)))))
  (func (export "type-f32") (block (drop (f32.neg (br 0)))))
  (func (export "type-f64") (block (drop (f64.neg (br 0)))))
  (func (export "type-i32-i32") (block (drop (i32.add (br 0)))))
  (func (export "type-i64-i64") (block (drop (i64.add (br 0)))))
  (func (export "type-f32-f32") (block (drop (f32.add (br 0)))))
  (func (export "type-f64-f64") (block (drop (f64.add (br 0)))))

  (func (export "type-i32-value") (result i32)
    (block (result i32) (i32.ctz (br 0 (i32.const 1))))
  )
  (func (export "type-i64-value") (result i64)
    (block (result i64) (i64.ctz (br 0 (i64.const 2))))
  )
  (func (export "type-f32-value") (result f32)
    (block (result f32) (f32.neg (br 0 (f32.const 3))))
  )
  (func (export "type-f64-value") (result f64)
    (block (result f64) (f64.neg (br 0 (f64.const 4))))
  )
  (func (export "type-f64-f64-value") (result f64 f64)
    (block (result f64 f64)
      (f64.add (br 0 (f64.const 4) (f64.const 5))) (f64.const 6)
    )
  )

  (func (export "as-block-first")
    (block (br 0) (call $dummy))
  )
  (func (export "as-block-mid")
    (block (call $dummy) (br 0) (call $dummy))
  )
  (func (export "as-block-last")
    (block (nop) (call $dummy) (br 0))
  )
  (func (export "as-block-value") (result i32)
    (block (result i32) (nop) (call $dummy) (br 0 (i32.const 2)))
  )

  (func (export "as-loop-first") (result i32)
    (block (result i32) (loop (result i32) (br 1 (i32.const 3)) (i32.const 2)))
  )
  (func (export "as-loop-mid") (result i32)
    (block (result i32)
      (loop (result i32) (call $dummy) (br 1 (i32.const 4)) (i32.const 2))
    )
  )
  (func (export "as-loop-last") (result i32)
    (block (result i32)
      (loop (result i32) (nop) (call $dummy) (br 1 (i32.const 5)))
    )
  )

  (func (export "as-br-value") (result i32)
    (block (result i32) (br 0 (br 0 (i32.const 9))))
  )

  (func (export "as-br_if-cond")
    (block (br_if 0 (br 0)))
  )
  (func (export "as-br_if-value") (result i32)
    (block (result i32)
      (drop (br_if 0 (br 0 (i32.const 8)) (i32.const 1))) (i32.const 7)
    )
  )
  (func (export "as-br_if-value-cond") (result i32)
    (block (result i32)
      (drop (br_if 0 (i32.const 6) (br 0 (i32.const 9)))) (i32.const 7)
    )
  )

  (func (export "as-br_table-index")
    (block (br_table 0 0 0 (br 0)))
  )
  (func (export "as-br_table-value") (result i32)
    (block (result i32)
      (br_table 0 0 0 (br 0 (i32.const 10)) (i32.const 1)) (i32.const 7)
    )
  )
  (func (export "as-br_table-value-index") (result i32)
    (block (result i32)
      (br_table 0 0 (i32.const 6) (br 0 (i32.const 11))) (i32.const 7)
    )
  )

  (func (export "as-return-value") (result i64)
    (block (result i64) (return (br 0 (i64.const 7))))
  )
  (func (export "as-return-values") (result i32 i64)
    (i32.const 2)
    (block (result i64) (return (br 0 (i32.const 1) (i64.const 7))))
  )

  (func (export "as-if-cond") (result i32)
    (block (result i32)
      (if (result i32) (br 0 (i32.const 2))
        (then (i32.const 0))
        (else (i32.const 1))
      )
    )
  )
  (func (export "as-if-then") (param i32 i32) (result i32)
    (block (result i32)
      (if (result i32) (local.get 0)
        (then (br 1 (i32.const 3)))
        (else (local.get 1))
      )
    )
  )
  (func (export "as-if-else") (param i32 i32) (result i32)
    (block (result i32)
      (if (result i32) (local.get 0)
        (then (local.get 1))
        (else (br 1 (i32.const 4)))
      )
    )
  )

  (func (export "as-select-first") (param i32 i32) (result i32)
    (block (result i32)
      (select (br 0 (i32.const 5)) (local.get 0) (local.get 1))
    )
  )
  (func (export "as-select-second") (param i32 i32) (result i32)
    (block (result i32)
      (select (local.get 0) (br 0 (i32.const 6)) (local.get 1))
    )
  )
  (func (export "as-select-cond") (result i32)
    (block (result i32)
      (select (i32.const 0) (i32.const 1) (br 0 (i32.const 7)))
    )
  )
  (func (export "as-select-all") (result i32)
    (block (result i32) (select (br 0 (i32.const 8))))
  )

  (func $f (param i32 i32 i32) (result i32) (i32.const -1))
  (func (export "as-call-first") (result i32)
    (block (result i32)
      (call $f (br 0 (i32.const 12)) (i32.const 2) (i32.const 3))
    )
  )
  (func (export "as-call-mid") (result i32)
    (block (result i32)
      (call $f (i32.const 1) (br 0 (i32.const 13)) (i32.const 3))
    )
  )
  (func (export "as-call-last") (result i32)
    (block (result i32)
      (call $f (i32.const 1) (i32.const 2) (br 0 (i32.const 14)))
    )
  )
  (func (export "as-call-all") (result i32)
    (block (result i32) (call $f (br 0 (i32.const 15))))
  )

  (type $sig (func (param i32 i32 i32) (result i32)))
  (table funcref (elem $f))
  (func (export "as-call_indirect-func") (result i32)
    (block (result i32)
      (call_indirect (type $sig)
        (br 0 (i32.const 20))
        (i32.const 1) (i32.const 2) (i32.const 3)
      )
    )
  )
  (func (export "as-call_indirect-first") (result i32)
    (block (result i32)
      (call_indirect (type $sig)
        (i32.const 0)
        (br 0 (i32.const 21)) (i32.const 2) (i32.const 3)
      )
    )
  )
  (func (export "as-call_indirect-mid") (result i32)
    (block (result i32)
      (call_indirect (type $sig)
        (i32.const 0)
        (i32.const 1) (br 0 (i32.const 22)) (i32.const 3)
      )
    )
  )
  (func (export "as-call_indirect-last") (result i32)
    (block (result i32)
      (call_indirect (type $sig)
        (i32.const 0)
        (i32.const 1) (i32.const 2) (br 0 (i32.const 23))
      )
    )
  )
  (func (export "as-call_indirect-all") (result i32)
    (block (result i32) (call_indirect (type $sig) (br 0 (i32.const 24))))
  )

  (func (export "as-local.set-value") (result i32) (local f32)
    (block (result i32) (local.set 0 (br 0 (i32.const 17))) (i32.const -1))
  )
  (func (export "as-local.tee-value") (result i32) (local i32)
    (block (result i32) (local.tee 0 (br 0 (i32.const 1))))
  )
  (global $a (mut i32) (i32.const 10))
  (func (export "as-global.set-value") (result i32)
    (block (result i32) (global.set $a (br 0 (i32.const 1))))
  )

  (memory 1)
  (func (export "as-load-address") (result f32)
    (block (result f32) (f32.load (br 0 (f32.const 1.7))))
  )
  (func (export "as-loadN-address") (result i64)
    (block (result i64) (i64.load8_s (br 0 (i64.const 30))))
  )

  (func (export "as-store-address") (result i32)
    (block (result i32)
      (f64.store (br 0 (i32.const 30)) (f64.const 7)) (i32.const -1)
    )
  )
  (func (export "as-store-value") (result i32)
    (block (result i32)
      (i64.store (i32.const 2) (br 0 (i32.const 31))) (i32.const -1)
    )
  )
  (func (export "as-store-both") (result i32)
    (block (result i32)
      (i64.store (br 0 (i32.const 32))) (i32.const -1)
    )
  )

  (func (export "as-storeN-address") (result i32)
    (block (result i32)
      (i32.store8 (br 0 (i32.const 32)) (i32.const 7)) (i32.const -1)
    )
  )
  (func (export "as-storeN-value") (result i32)
    (block (result i32)
      (i64.store16 (i32.const 2) (br 0 (i32.const 33))) (i32.const -1)
    )
  )
  (func (export "as-storeN-both") (result i32)
    (block (result i32)
      (i64.store16 (br 0 (i32.const 34))) (i32.const -1)
    )
  )

  (func (export "as-unary-operand") (result f32)
    (block (result f32) (f32.neg (br 0 (f32.const 3.4))))
  )

  (func (export "as-binary-left") (result i32)
    (block (result i32) (i32.add (br 0 (i32.const 3)) (i32.const 10)))
  )
  (func (export "as-binary-right") (result i64)
    (block (result i64) (i64.sub (i64.const 10) (br 0 (i64.const 45))))
  )
  (func (export "as-binary-both") (result i32)
    (block (result i32) (i32.add (br 0 (i32.const 46))))
  )

  (func (export "as-test-operand") (result i32)
    (block (result i32) (i32.eqz (br 0 (i32.const 44))))
  )

  (func (export "as-compare-left") (result i32)
    (block (result i32) (f64.le (br 0 (i32.const 43)) (f64.const 10)))
  )
  (func (export "as-compare-right") (result i32)
    (block (result i32) (f32.ne (f32.const 10) (br 0 (i32.const 42))))
  )
  (func (export "as-compare-both") (result i32)
    (block (result i32) (f64.le (br 0 (i32.const 44))))
  )

  (func (export "as-convert-operand") (result i32)
    (block (result i32) (i32.wrap_i64 (br 0 (i32.const 41))))
  )

  (func (export "as-memory.grow-size") (result i32)
    (block (result i32) (memory.grow (br 0 (i32.const 40))))
  )

  (func (export "nested-block-value") (result i32)
    (i32.add
      (i32.const 1)
      (block (result i32)
        (call $dummy)
        (i32.add (i32.const 4) (br 0 (i32.const 8)))
      )
    )
  )

  (func (export "nested-br-value") (result i32)
    (i32.add
      (i32.const 1)
      (block (result i32)
        (drop (i32.const 2))
        (drop
          (block (result i32)
            (drop (i32.const 4))
            (br 0 (br 1 (i32.const 8)))
          )
        )
        (i32.const 16)
      )
    )
  )

  (func (export "nested-br_if-value") (result i32)
    (i32.add
      (i32.const 1)
      (block (result i32)
        (drop (i32.const 2))
        (drop
          (block (result i32)
            (drop (i32.const 4))
            (drop (br_if 0 (br 1 (i32.const 8)) (i32.const 1)))
            (i32.const 32)
          )
        )
        (i32.const 16)
      )
    )
  )

  (func (export "nested-br_if-value-cond") (result i32)
    (i32.add
      (i32.const 1)
      (block (result i32)
        (drop (i32.const 2))
        (drop (br_if 0 (i32.const 4) (br 0 (i32.const 8))))
        (i32.const 16)
      )
    )
  )

  (func (export "nested-br_table-value") (result i32)
    (i32.add
      (i32.const 1)
      (block (result i32)
        (drop (i32.const 2))
        (drop
          (block (result i32)
            (drop (i32.const 4))
            (br_table 0 (br 1 (i32.const 8)) (i32.const 1))
          )
        )
        (i32.const 16)
      )
    )
  )

  (func (export "nested-br_table-value-index") (result i32)
    (i32.add
      (i32.const 1)
      (block (result i32)
        (drop (i32.const 2))
        (br_table 0 (i32.const 4) (br 0 (i32.const 8)))
        (i32.const 16)
      )
    )
  )
)

(assert_return (invoke "type-i32"))
(assert_return (invoke "type-i64"))
(assert_return (invoke "type-f32"))
(assert_return (invoke "type-f64"))
(assert_return (invoke "type-i32-i32"))
(assert_return (invoke "type-i64-i64"))
(assert_return (invoke "type-f32-f32"))
(assert_return (invoke "type-f64-f64"))

(assert_return (invoke "type-i32-value") (i32.const 1))
(assert_return (invoke "type-i64-value") (i64.const 2))
(assert_return (invoke "type-f32-value") (f32.const 3))
(assert_return (invoke "type-f64-value") (f64.const 4))
(assert_return (invoke "type-f64-f64-value") (f64.const 4) (f64.const 5))

(assert_return (invoke "as-block-first"))
(assert_return (invoke "as-block-mid"))
(assert_return (invoke "as-block-last"))
(assert_return (invoke "as-block-value") (i32.const 2))

(assert_return (invoke "as-loop-first") (i32.const 3))
(assert_return (invoke "as-loop-mid") (i32.const 4))
(assert_return (invoke "as-loop-last") (i32.const 5))

(assert_return (invoke "as-br-value") (i32.const 9))

(assert_return (invoke "as-br_if-cond"))
(assert_return (invoke "as-br_if-value") (i32.const 8))
(assert_return (invoke "as-br_if-value-cond") (i32.const 9))

(assert_return (invoke "as-br_table-index"))
(assert_return (invoke "as-br_table-value") (i32.const 10))
(assert_return (invoke "as-br_table-value-index") (i32.const 11))

(assert_return (invoke "as-return-value") (i64.const 7))
(assert_return (invoke "as-return-values") (i32.const 2) (i64.const 7))

(assert_return (invoke "as-if-cond") (i32.const 2))
(assert_return (invoke "as-if-then" (i32.const 1) (i32.const 6)) (i32.const 3))
(assert_return (invoke "as-if-then" (i32.const 0) (i32.const 6)) (i32.const 6))
(assert_return (invoke "as-if-else" (i32.const 0) (i32.const 6)) (i32.const 4))
(assert_return (invoke "as-if-else" (i32.const 1) (i32.const 6)) (i32.const 6))

(assert_return (invoke "as-select-first" (i32.const 0) (i32.const 6)) (i32.const 5))
(assert_return (invoke "as-select-first" (i32.const 1) (i32.const 6)) (i32.const 5))
(assert_return (invoke "as-select-second" (i32.const 0) (i32.const 6)) (i32.const 6))
(assert_return (invoke "as-select-second" (i32.const 1) (i32.const 6)) (i32.const 6))
(assert_return (invoke "as-select-cond") (i32.const 7))
(assert_return (invoke "as-select-all") (i32.const 8))

(assert_return (invoke "as-call-first") (i32.const 12))
(assert_return (invoke "as-call-mid") (i32.const 13))
(assert_return (invoke "as-call-last") (i32.const 14))
(assert_return (invoke "as-call-all") (i32.const 15))

(assert_return (invoke "as-call_indirect-func") (i32.const 20))
(assert_return (invoke "as-call_indirect-first") (i32.const 21))
(assert_return (invoke "as-call_indirect-mid") (i32.const 22))
(assert_return (invoke "as-call_indirect-last") (i32.const 23))
(assert_return (invoke "as-call_indirect-all") (i32.const 24))

(assert_return (invoke "as-local.set-value") (i32.const 17))
(assert_return (invoke "as-local.tee-value") (i32.const 1))
(assert_return (invoke "as-global.set-value") (i32.const 1))

(assert_return (invoke "as-load-address") (f32.const 1.7))
(assert_return (invoke "as-loadN-address") (i64.const 30))

(assert_return (invoke "as-store-address") (i32.const 30))
(assert_return (invoke "as-store-value") (i32.const 31))
(assert_return (invoke "as-store-both") (i32.const 32))
(assert_return (invoke "as-storeN-address") (i32.const 32))
(assert_return (invoke "as-storeN-value") (i32.const 33))
(assert_return (invoke "as-storeN-both") (i32.const 34))

(assert_return (invoke "as-unary-operand") (f32.const 3.4))

(assert_return (invoke "as-binary-left") (i32.const 3))
(assert_return (invoke "as-binary-right") (i64.const 45))
(assert_return (invoke "as-binary-both") (i32.const 46))

(assert_return (invoke "as-test-operand") (i32.const 44))

(assert_return (invoke "as-compare-left") (i32.const 43))
(assert_return (invoke "as-compare-right") (i32.const 42))
(assert_return (invoke "as-compare-both") (i32.const 44))

(assert_return (invoke "as-convert-operand") (i32.const 41))

(assert_return (invoke "as-memory.grow-size") (i32.const 40))

(assert_return (invoke "nested-block-value") (i32.const 9))
(assert_return (invoke "nested-br-value") (i32.const 9))
(assert_return (invoke "nested-br_if-value") (i32.const 9))
(assert_return (invoke "nested-br_if-value-cond") (i32.const 9))
(assert_return (invoke "nested-br_table-value") (i32.const 9))
(assert_return (invoke "nested-br_table-value-index") (i32.const 9))

(assert_invalid
  (module (func $type-arg-empty-vs-num (result i32)
    (block (result i32) (br 0) (i32.const 1))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-arg-void-vs-num (result i32)
    (block (result i32) (br 0 (nop)) (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-arg-void-vs-num-nested (result i32)
    (block (result i32) (i32.const 0) (block (br 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-arg-num-vs-num (result i32)
    (block (result i32) (br 0 (i64.const 1)) (i32.const 1))
  ))
  "type mismatch"
)

(assert_invalid
  (module
    (func $type-arg-empty-in-br
      (i32.const 0)
      (block (result i32) (br 0 (br 0))) (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-arg-empty-in-br_if
      (i32.const 0)
      (block (result i32) (br_if 0 (br 0) (i32.const 1))) (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-arg-empty-in-br_table
      (i32.const 0)
      (block (result i32) (br_table 0 (br 0))) (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-arg-empty-in-return
      (block (result i32)
        (return (br 0))
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-arg-empty-in-select
      (block (result i32)
        (select (br 0) (i32.const 1) (i32.const 2))
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-arg-empty-in-call
      (block (result i32)
        (call 1 (br 0))
      )
      (i32.eqz) (drop)
    )
    (func (param i32) (result i32) (local.get 0))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $f (param i32) (result i32) (local.get 0))
    (type $sig (func (param i32) (result i32)))
    (table funcref (elem $f))
    (func $type-arg-empty-in-call_indirect
      (block (result i32)
        (call_indirect (type $sig)
          (br 0) (i32.const 0)
        )
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-arg-empty-in-local.set
      (local i32)
      (block (result i32)
        (local.set 0 (br 0)) (local.get 0)
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-arg-empty-in-local.tee
      (local i32)
      (block (result i32)
        (local.tee 0 (br 0))
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-arg-empty-in-global.set
      (block (result i32)
        (global.set $x (br 0)) (global.get $x)
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 0)
    (func $type-arg-empty-in-memory.grow
      (block (result i32)
        (memory.grow (br 0))
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 1)
    (func $type-arg-empty-in-load
      (block (result i32)
        (i32.load (br 0))
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 1)
    (func $type-arg-empty-in-store
      (block (result i32)
        (i32.store (br 0) (i32.const 0))
      )
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)

(assert_invalid
  (module (func $unbound-label (br 1)))
  "unknown label"
)
(assert_invalid
  (module (func $unbound-nested-label (block (block (br 5)))))
  "unknown label"
)
(assert_invalid
  (module (func $large-label (br 0x10000001)))
  "unknown label"
)
