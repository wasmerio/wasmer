;; Test `block` operator

(module
  ;; Auxiliary definition
  (memory 1)

  (func $dummy)

  (func (export "empty")
    (block)
    (block $l)
  )

  (func (export "singular") (result i32)
    (block (nop))
    (block (result i32) (i32.const 7))
  )

  (func (export "multi") (result i32)
    (block (call $dummy) (call $dummy) (call $dummy) (call $dummy))
    (block (result i32)
      (call $dummy) (call $dummy) (call $dummy) (i32.const 7) (call $dummy)
    )
    (drop)
    (block (result i32 i64 i32)
      (call $dummy) (call $dummy) (call $dummy) (i32.const 8) (call $dummy)
      (call $dummy) (call $dummy) (call $dummy) (i64.const 7) (call $dummy)
      (call $dummy) (call $dummy) (call $dummy) (i32.const 9) (call $dummy)
    )
    (drop) (drop)
  )

  (func (export "nested") (result i32)
    (block (result i32)
      (block (call $dummy) (block) (nop))
      (block (result i32) (call $dummy) (i32.const 9))
    )
  )

  (func (export "deep") (result i32)
    (block (result i32) (block (result i32)
      (block (result i32) (block (result i32)
        (block (result i32) (block (result i32)
          (block (result i32) (block (result i32)
            (block (result i32) (block (result i32)
              (block (result i32) (block (result i32)
                (block (result i32) (block (result i32)
                  (block (result i32) (block (result i32)
                    (block (result i32) (block (result i32)
                      (block (result i32) (block (result i32)
                        (block (result i32) (block (result i32)
                          (block (result i32) (block (result i32)
                            (block (result i32) (block (result i32)
                              (block (result i32) (block (result i32)
                                (block (result i32) (block (result i32)
                                  (block (result i32) (block (result i32)
                                    (block (result i32) (block (result i32)
                                      (block (result i32) (block (result i32)
                                        (block (result i32) (block (result i32)
                                          (call $dummy) (i32.const 150)
                                        ))
                                      ))
                                    ))
                                  ))
                                ))
                              ))
                            ))
                          ))
                        ))
                      ))
                    ))
                  ))
                ))
              ))
            ))
          ))
        ))
      ))
    ))
  )

  (func (export "as-select-first") (result i32)
    (select (block (result i32) (i32.const 1)) (i32.const 2) (i32.const 3))
  )
  (func (export "as-select-mid") (result i32)
    (select (i32.const 2) (block (result i32) (i32.const 1)) (i32.const 3))
  )
  (func (export "as-select-last") (result i32)
    (select (i32.const 2) (i32.const 3) (block (result i32) (i32.const 1)))
  )

  (func (export "as-loop-first") (result i32)
    (loop (result i32) (block (result i32) (i32.const 1)) (call $dummy) (call $dummy))
  )
  (func (export "as-loop-mid") (result i32)
    (loop (result i32) (call $dummy) (block (result i32) (i32.const 1)) (call $dummy))
  )
  (func (export "as-loop-last") (result i32)
    (loop (result i32) (call $dummy) (call $dummy) (block (result i32) (i32.const 1)))
  )

  (func (export "as-if-condition")
    (block (result i32) (i32.const 1)) (if (then (call $dummy)))
  )
  (func (export "as-if-then") (result i32)
    (if (result i32) (i32.const 1) (then (block (result i32) (i32.const 1))) (else (i32.const 2)))
  )
  (func (export "as-if-else") (result i32)
    (if (result i32) (i32.const 1) (then (i32.const 2)) (else (block (result i32) (i32.const 1))))
  )

  (func (export "as-br_if-first") (result i32)
    (block (result i32) (br_if 0 (block (result i32) (i32.const 1)) (i32.const 2)))
  )
  (func (export "as-br_if-last") (result i32)
    (block (result i32) (br_if 0 (i32.const 2) (block (result i32) (i32.const 1))))
  )

  (func (export "as-br_table-first") (result i32)
    (block (result i32) (block (result i32) (i32.const 1)) (i32.const 2) (br_table 0 0))
  )
  (func (export "as-br_table-last") (result i32)
    (block (result i32) (i32.const 2) (block (result i32) (i32.const 1)) (br_table 0 0))
  )

  (func $func (param i32 i32) (result i32) (local.get 0))
  (type $check (func (param i32 i32) (result i32)))
  (table funcref (elem $func))
  (func (export "as-call_indirect-first") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (block (result i32) (i32.const 1)) (i32.const 2) (i32.const 0)
      )
    )
  )
  (func (export "as-call_indirect-mid") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (i32.const 2) (block (result i32) (i32.const 1)) (i32.const 0)
      )
    )
  )
  (func (export "as-call_indirect-last") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (i32.const 1) (i32.const 2) (block (result i32) (i32.const 0))
      )
    )
  )

  (func (export "as-store-first")
    (block (result i32) (i32.const 1)) (i32.const 1) (i32.store)
  )
  (func (export "as-store-last")
    (i32.const 10) (block (result i32) (i32.const 1)) (i32.store)
  )

  (func (export "as-memory.grow-value") (result i32)
    (memory.grow (block (result i32) (i32.const 1)))
  )

  (func $f (param i32) (result i32) (local.get 0))

  (func (export "as-call-value") (result i32)
    (call $f (block (result i32) (i32.const 1)))
  )
  (func (export "as-return-value") (result i32)
    (block (result i32) (i32.const 1)) (return)
  )
  (func (export "as-drop-operand")
    (drop (block (result i32) (i32.const 1)))
  )
  (func (export "as-br-value") (result i32)
    (block (result i32) (br 0 (block (result i32) (i32.const 1))))
  )
  (func (export "as-local.set-value") (result i32)
    (local i32) (local.set 0 (block (result i32) (i32.const 1))) (local.get 0)
  )
  (func (export "as-local.tee-value") (result i32)
    (local i32) (local.tee 0 (block (result i32) (i32.const 1)))
  )
  (global $a (mut i32) (i32.const 10))
  (func (export "as-global.set-value") (result i32)
    (global.set $a (block (result i32) (i32.const 1)))
    (global.get $a)
  )

  (func (export "as-load-operand") (result i32)
    (i32.load (block (result i32) (i32.const 1)))
  )

  (func (export "as-unary-operand") (result i32)
    (i32.ctz (block (result i32) (call $dummy) (i32.const 13)))
  )
  (func (export "as-binary-operand") (result i32)
    (i32.mul
      (block (result i32) (call $dummy) (i32.const 3))
      (block (result i32) (call $dummy) (i32.const 4))
    )
  )
  (func (export "as-test-operand") (result i32)
    (i32.eqz (block (result i32) (call $dummy) (i32.const 13)))
  )
  (func (export "as-compare-operand") (result i32)
    (f32.gt
      (block (result f32) (call $dummy) (f32.const 3))
      (block (result f32) (call $dummy) (f32.const 3))
    )
  )
  (func (export "as-binary-operands") (result i32)
    (i32.mul
      (block (result i32 i32)
        (call $dummy) (i32.const 3) (call $dummy) (i32.const 4)
      )
    )
  )
  (func (export "as-compare-operands") (result i32)
    (f32.gt
      (block (result f32 f32)
        (call $dummy) (f32.const 3) (call $dummy) (f32.const 3)
      )
    )
  )
  (func (export "as-mixed-operands") (result i32)
    (block (result i32 i32)
      (call $dummy) (i32.const 3) (call $dummy) (i32.const 4)
    )
    (i32.const 5)
    (i32.add)
    (i32.mul)
  )

  (func (export "break-bare") (result i32)
    (block (br 0) (unreachable))
    (block (br_if 0 (i32.const 1)) (unreachable))
    (block (br_table 0 (i32.const 0)) (unreachable))
    (block (br_table 0 0 0 (i32.const 1)) (unreachable))
    (i32.const 19)
  )
  (func (export "break-value") (result i32)
    (block (result i32) (br 0 (i32.const 18)) (i32.const 19))
  )
  (func (export "break-multi-value") (result i32 i32 i64)
    (block (result i32 i32 i64)
      (br 0 (i32.const 18) (i32.const -18) (i64.const 18))
      (i32.const 19) (i32.const -19) (i64.const 19)
    )
  )
  (func (export "break-repeated") (result i32)
    (block (result i32)
      (br 0 (i32.const 18))
      (br 0 (i32.const 19))
      (drop (br_if 0 (i32.const 20) (i32.const 0)))
      (drop (br_if 0 (i32.const 20) (i32.const 1)))
      (br 0 (i32.const 21))
      (br_table 0 (i32.const 22) (i32.const 4))
      (br_table 0 0 0 (i32.const 23) (i32.const 1))
      (i32.const 21)
    )
  )
  (func (export "break-inner") (result i32)
    (local i32)
    (local.set 0 (i32.const 0))
    (local.set 0 (i32.add (local.get 0) (block (result i32) (block (result i32) (br 1 (i32.const 0x1))))))
    (local.set 0 (i32.add (local.get 0) (block (result i32) (block (br 0)) (i32.const 0x2))))
    (local.set 0
      (i32.add (local.get 0) (block (result i32) (i32.ctz (br 0 (i32.const 0x4)))))
    )
    (local.set 0
      (i32.add (local.get 0) (block (result i32) (i32.ctz (block (result i32) (br 1 (i32.const 0x8))))))
    )
    (local.get 0)
  )

  (func (export "param") (result i32)
    (i32.const 1)
    (block (param i32) (result i32)
      (i32.const 2)
      (i32.add)
    )
  )
  (func (export "params") (result i32)
    (i32.const 1)
    (i32.const 2)
    (block (param i32 i32) (result i32)
      (i32.add)
    )
  )
  (func (export "params-id") (result i32)
    (i32.const 1)
    (i32.const 2)
    (block (param i32 i32) (result i32 i32))
    (i32.add)
  )
  (func (export "param-break") (result i32)
    (i32.const 1)
    (block (param i32) (result i32)
      (i32.const 2)
      (i32.add)
      (br 0)
    )
  )
  (func (export "params-break") (result i32)
    (i32.const 1)
    (i32.const 2)
    (block (param i32 i32) (result i32)
      (i32.add)
      (br 0)
    )
  )
  (func (export "params-id-break") (result i32)
    (i32.const 1)
    (i32.const 2)
    (block (param i32 i32) (result i32 i32) (br 0))
    (i32.add)
  )

  (func (export "effects") (result i32)
    (local i32)
    (block
      (local.set 0 (i32.const 1))
      (local.set 0 (i32.mul (local.get 0) (i32.const 3)))
      (local.set 0 (i32.sub (local.get 0) (i32.const 5)))
      (local.set 0 (i32.mul (local.get 0) (i32.const 7)))
      (br 0)
      (local.set 0 (i32.mul (local.get 0) (i32.const 100)))
    )
    (i32.eq (local.get 0) (i32.const -14))
  )

  (type $block-sig-1 (func))
  (type $block-sig-2 (func (result i32)))
  (type $block-sig-3 (func (param $x i32)))
  (type $block-sig-4 (func (param i32 f64 i32) (result i32 f64 i32)))

  (func (export "type-use")
    (block (type $block-sig-1))
    (block (type $block-sig-2) (i32.const 0))
    (block (type $block-sig-3) (drop))
    (i32.const 0) (f64.const 0) (i32.const 0)
    (block (type $block-sig-4))
    (drop) (drop) (drop)
    (block (type $block-sig-2) (result i32) (i32.const 0))
    (block (type $block-sig-3) (param i32) (drop))
    (i32.const 0) (f64.const 0) (i32.const 0)
    (block (type $block-sig-4)
      (param i32) (param f64 i32) (result i32 f64) (result i32)
    )
    (drop) (drop) (drop)
  )
)

(assert_return (invoke "empty"))
(assert_return (invoke "singular") (i32.const 7))
(assert_return (invoke "multi") (i32.const 8))
(assert_return (invoke "nested") (i32.const 9))
(assert_return (invoke "deep") (i32.const 150))

(assert_return (invoke "as-select-first") (i32.const 1))
(assert_return (invoke "as-select-mid") (i32.const 2))
(assert_return (invoke "as-select-last") (i32.const 2))

(assert_return (invoke "as-loop-first") (i32.const 1))
(assert_return (invoke "as-loop-mid") (i32.const 1))
(assert_return (invoke "as-loop-last") (i32.const 1))

(assert_return (invoke "as-if-condition"))
(assert_return (invoke "as-if-then") (i32.const 1))
(assert_return (invoke "as-if-else") (i32.const 2))

(assert_return (invoke "as-br_if-first") (i32.const 1))
(assert_return (invoke "as-br_if-last") (i32.const 2))

(assert_return (invoke "as-br_table-first") (i32.const 1))
(assert_return (invoke "as-br_table-last") (i32.const 2))

(assert_return (invoke "as-call_indirect-first") (i32.const 1))
(assert_return (invoke "as-call_indirect-mid") (i32.const 2))
(assert_return (invoke "as-call_indirect-last") (i32.const 1))

(assert_return (invoke "as-store-first"))
(assert_return (invoke "as-store-last"))

(assert_return (invoke "as-memory.grow-value") (i32.const 1))
(assert_return (invoke "as-call-value") (i32.const 1))
(assert_return (invoke "as-return-value") (i32.const 1))
(assert_return (invoke "as-drop-operand"))
(assert_return (invoke "as-br-value") (i32.const 1))
(assert_return (invoke "as-local.set-value") (i32.const 1))
(assert_return (invoke "as-local.tee-value") (i32.const 1))
(assert_return (invoke "as-global.set-value") (i32.const 1))
(assert_return (invoke "as-load-operand") (i32.const 1))

(assert_return (invoke "as-unary-operand") (i32.const 0))
(assert_return (invoke "as-binary-operand") (i32.const 12))
(assert_return (invoke "as-test-operand") (i32.const 0))
(assert_return (invoke "as-compare-operand") (i32.const 0))
(assert_return (invoke "as-binary-operands") (i32.const 12))
(assert_return (invoke "as-compare-operands") (i32.const 0))
(assert_return (invoke "as-mixed-operands") (i32.const 27))

(assert_return (invoke "break-bare") (i32.const 19))
(assert_return (invoke "break-value") (i32.const 18))
(assert_return (invoke "break-multi-value")
  (i32.const 18) (i32.const -18) (i64.const 18)
)
(assert_return (invoke "break-repeated") (i32.const 18))
(assert_return (invoke "break-inner") (i32.const 0xf))

(assert_return (invoke "param") (i32.const 3))
(assert_return (invoke "params") (i32.const 3))
(assert_return (invoke "params-id") (i32.const 3))
(assert_return (invoke "param-break") (i32.const 3))
(assert_return (invoke "params-break") (i32.const 3))
(assert_return (invoke "params-id-break") (i32.const 3))

(assert_return (invoke "effects") (i32.const 1))

(assert_return (invoke "type-use"))

(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (i32.const 0) (block (type $sig) (result i32) (param i32)))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (i32.const 0) (block (param i32) (type $sig) (result i32)))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (i32.const 0) (block (param i32) (result i32) (type $sig)))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (i32.const 0) (block (result i32) (type $sig) (param i32)))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (i32.const 0) (block (result i32) (param i32) (type $sig)))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(func (i32.const 0) (block (result i32) (param i32)))"
  )
  "unexpected token"
)

(assert_malformed
  (module quote "(func (i32.const 0) (block (param $x i32) (drop)))")
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func))"
    "(func (block (type $sig) (result i32) (i32.const 0)) (unreachable))"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (block (type $sig) (result i32) (i32.const 0)) (unreachable))"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (i32.const 0) (block (type $sig) (param i32) (drop)) (unreachable))"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32 i32) (result i32)))"
    "(func (i32.const 0) (block (type $sig) (param i32) (result i32)) (unreachable))"
  )
  "inline function type"
)

(assert_invalid
  (module
    (type $sig (func))
    (func (block (type $sig) (i32.const 0)))
  )
  "type mismatch"
)

(assert_invalid
  (module (func $type-empty-i32 (result i32) (block)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-i64 (result i64) (block)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f32 (result f32) (block)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f64 (result f64) (block)))
  "type mismatch"
)

(assert_invalid
  (module (func $type-value-i32-vs-void
    (block (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-i64-vs-void
    (block (i64.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-f32-vs-void
    (block (f32.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-f64-vs-void
    (block (f64.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-nums-vs-void
    (block (i32.const 1) (i32.const 2))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-empty-vs-i32 (result i32)
    (block (result i32))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-empty-vs-i64 (result i64)
    (block (result i64))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-empty-vs-f32 (result f32)
    (block (result f32))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-empty-vs-f64 (result f64)
    (block (result f64))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-empty-vs-nums (result i32 i32)
    (block (result i32 i32))
  ))
  "type mismatch"
)

(assert_invalid
  (module
    (func $type-value-empty-in-block
      (i32.const 0)
      (block (block (result i32)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-value-empty-in-loop
      (i32.const 0)
      (loop (block (result i32)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-value-empty-in-then
      (i32.const 0) (i32.const 0)
      (if (then (block (result i32)) (drop)))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module (func $type-value-void-vs-i32 (result i32)
    (block (result i32) (nop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-void-vs-i64 (result i64)
    (block (result i64) (nop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-void-vs-f32 (result f32)
    (block (result f32) (nop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-void-vs-f64 (result f64)
    (block (result f64) (nop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-void-vs-nums (result i32 i32)
    (block (result i32 i32) (nop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-i32-vs-i64 (result i32)
    (block (result i32) (i64.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-i32-vs-f32 (result i32)
    (block (result i32) (f32.const 0.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-i32-vs-f64 (result i32)
    (block (result i32) (f64.const 0.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-i64-vs-i32 (result i64)
    (block (result i64) (i32.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-i64-vs-f32 (result i64)
    (block (result i64) (f32.const 0.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-i64-vs-f64 (result i64)
    (block (result i64) (f64.const 0.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-f32-vs-i32 (result f32)
    (block (result f32) (i32.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-f32-vs-i64 (result f32)
    (block (result f32) (i64.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-f32-vs-f64 (result f32)
    (block (result f32) (f64.const 0.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-f64-vs-i32 (result f64)
    (block (result f64) (i32.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-f64-vs-i64 (result f64)
    (block (result f64) (i64.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-f64-vs-f32 (result f32)
    (block (result f64) (f32.const 0.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-num-vs-nums (result i32 i32)
    (block (result i32 i32) (i32.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-partial-vs-nums (result i32 i32)
    (i32.const 1) (block (result i32 i32) (i32.const 2))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-nums-vs-num (result i32)
    (block (result i32) (i32.const 1) (i32.const 2))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-value-unreached-select-i32-i64 (result i32)
    (block (result i64) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-i32-f32 (result i32)
    (block (result f32) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-i32-f64 (result i32)
    (block (result f64) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-i64-i32 (result i64)
    (block (result i32) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-i64-f32 (result i64)
    (block (result f32) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-i64-f64 (result i64)
    (block (result f64) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-f32-i32 (result f32)
    (block (result i32) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-f32-i64 (result f32)
    (block (result i64) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-f32-f64 (result f32)
    (block (result f64) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-f64-i32 (result f64)
    (block (result i32) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-f64-i64 (result f64)
    (block (result i64) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-unreached-select-f64-f32 (result f64)
    (block (result f32) (select (unreachable) (unreachable) (unreachable)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-last-void-vs-i32 (result i32)
    (block (result i32) (br 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-last-void-vs-i64 (result i64)
    (block (result i64) (br 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-last-void-vs-f32 (result f32)
    (block (result f32) (br 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-last-void-vs-f64 (result f64)
    (block (result f64) (br 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-last-void-vs-nums (result i32 i32)
    (block (result i32 i32) (br 0))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-empty-vs-i32 (result i32)
    (block (result i32) (br 0) (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-empty-vs-i64 (result i64)
    (block (result i64) (br 0) (i64.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-empty-vs-f32 (result f32)
    (block (result f32) (br 0) (f32.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-empty-vs-f64 (result f64)
    (block (result f64) (br 0) (f64.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-empty-vs-nums (result i32 i32)
    (block (result i32 i32) (br 0) (i32.const 1) (i32.const 2))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-void-vs-i32 (result i32)
    (block (result i32) (br 0 (nop)) (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-void-vs-i64 (result i64)
    (block (result i64) (br 0 (nop)) (i64.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-void-vs-f32 (result f32)
    (block (result f32) (br 0 (nop)) (f32.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-void-vs-f64 (result f64)
    (block (result f64) (br 0 (nop)) (f64.const 1.0))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-i32-vs-i64 (result i32)
    (block (result i32) (br 0 (i64.const 1)) (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-i32-vs-f32 (result i32)
    (block (result i32) (br 0 (f32.const 1.0)) (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-i32-vs-f64 (result i32)
    (block (result i32) (br 0 (f64.const 1.0)) (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-i64-vs-i32 (result i64)
    (block (result i64) (br 0 (i32.const 1)) (i64.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-i64-vs-f32 (result i64)
    (block (result i64) (br 0 (f32.const 1.0)) (i64.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-i64-vs-f64 (result i64)
    (block (result i64) (br 0 (f64.const 1.0)) (i64.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-f32-vs-i32 (result f32)
    (block (result f32) (br 0 (i32.const 1)) (f32.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-f32-vs-i64 (result f32)
    (block (result f32) (br 0 (i64.const 1)) (f32.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-f32-vs-f64 (result f32)
    (block (result f32) (br 0 (f64.const 1.0)) (f32.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-f64-vs-i32 (result f64)
    (block (result i64) (br 0 (i32.const 1)) (f64.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-f64-vs-i64 (result f64)
    (block (result f64) (br 0 (i64.const 1)) (f64.const 1.0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-f64-vs-f32 (result f64)
    (block (result f64) (br 0 (f32.const 1.0)) (f64.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-num-vs-nums (result i32 i32)
    (block (result i32 i32) (br 0 (i32.const 0)) (i32.const 1) (i32.const 2))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-partial-vs-nums (result i32 i32)
    (i32.const 1) (block (result i32 i32) (br 0 (i32.const 0)) (i32.const 2))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-first-void-vs-i32 (result i32)
    (block (result i32) (br 0 (nop)) (br 0 (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-void-vs-i64 (result i64)
    (block (result i64) (br 0 (nop)) (br 0 (i64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-void-vs-f32 (result f32)
    (block (result f32) (br 0 (nop)) (br 0 (f32.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-void-vs-f64 (result f64)
    (block (result f64) (br 0 (nop)) (br 0 (f64.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-void-vs-nums (result i32 i32)
    (block (result i32 i32) (br 0 (nop)) (br 0 (i32.const 1) (i32.const 2)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-first-i32-vs-i64 (result i32)
    (block (result i32) (br 0 (i64.const 1)) (br 0 (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-i32-vs-f32 (result i32)
    (block (result i32) (br 0 (f32.const 1.0)) (br 0 (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-i32-vs-f64 (result i32)
    (block (result i32) (br 0 (f64.const 1.0)) (br 0 (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-i64-vs-i32 (result i64)
    (block (result i64) (br 0 (i32.const 1)) (br 0 (i64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-i64-vs-f32 (result i64)
    (block (result i64) (br 0 (f32.const 1.0)) (br 0 (i64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-i64-vs-f64 (result i64)
    (block (result i64) (br 0 (f64.const 1.0)) (br 0 (i64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-f32-vs-i32 (result f32)
    (block (result f32) (br 0 (i32.const 1)) (br 0 (f32.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-f32-vs-i64 (result f32)
    (block (result f32) (br 0 (i64.const 1)) (br 0 (f32.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-f32-vs-f64 (result f32)
    (block (result f32) (br 0 (f64.const 1.0)) (br 0 (f32.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-f64-vs-i32 (result f64)
    (block (result f64) (br 0 (i32.const 1)) (br 0 (f64.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-f64-vs-i64 (result f64)
    (block (result f64) (br 0 (i64.const 1)) (br 0 (f64.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-f64-vs-f32 (result f64)
    (block (result f64) (br 0 (f32.const 1.0)) (br 0 (f64.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-num-vs-nums (result i32 i32)
    (block (result i32 i32) (br 0 (i32.const 0)) (br 0 (i32.const 1) (i32.const 2)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-nested-i32-vs-void
    (block (result i32) (block (result i32) (br 1 (i32.const 1))) (br 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-i64-vs-void
    (block (result i64) (block (result i64) (br 1 (i64.const 1))) (br 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-f32-vs-void
    (block (result f32) (block (result f32) (br 1 (f32.const 1.0))) (br 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-f64-vs-void
    (block (result f64) (block (result f64) (br 1 (f64.const 1.0))) (br 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-nums-vs-void
    (block (result i32 i32) (block (result i32 i32) (br 1 (i32.const 1) (i32.const 2))) (br 0))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-nested-empty-vs-i32 (result i32)
    (block (result i32) (block (br 1)) (br 0 (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-empty-vs-i64 (result i64)
    (block (result i64) (block (br 1)) (br 0 (i64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-empty-vs-f32 (result f32)
    (block (result f32) (block (br 1)) (br 0 (f32.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-empty-vs-f64 (result f64)
    (block (result f64) (block (br 1)) (br 0 (f64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-empty-vs-nums (result i32 i32)
    (block (result i32 i32) (block (br 1)) (br 0 (i32.const 1) (i32.const 2)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-nested-void-vs-i32 (result i32)
    (block (result i32) (block (result i32) (br 1 (nop))) (br 0 (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-void-vs-i64 (result i64)
    (block (result i64) (block (result i64) (br 1 (nop))) (br 0 (i64.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-void-vs-f32 (result f32)
    (block (result f32) (block (result f32) (br 1 (nop))) (br 0 (f32.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-void-vs-f64 (result f64)
    (block (result f64) (block (result f64) (br 1 (nop))) (br 0 (f64.const 1.0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-void-vs-nums (result i32 i32)
    (block (result i32 i32) (block (result i32 i32) (br 1 (nop))) (br 0 (i32.const 1) (i32.const 2)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-nested-i32-vs-i64 (result i32)
    (block (result i32)
      (block (result i32) (br 1 (i64.const 1))) (br 0 (i32.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-i32-vs-f32 (result i32)
    (block (result i32)
      (block (result i32) (br 1 (f32.const 1.0))) (br 0 (i32.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-i32-vs-f64 (result i32)
    (block (result i32)
      (block (result i32) (br 1 (f64.const 1.0))) (br 0 (i32.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-i64-vs-i32 (result i64)
    (block (result i64)
      (block (result i64) (br 1 (i32.const 1))) (br 0 (i64.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-i64-vs-f32 (result i64)
    (block (result i64)
      (block (result i64) (br 1 (f32.const 1.0))) (br 0 (i64.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-i64-vs-f64 (result i64)
    (block (result i64)
      (block (result i64) (br 1 (f64.const 1.0))) (br 0 (i64.const 1))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-f32-vs-i32 (result f32)
    (block (result f32)
      (block (result f32) (br 1 (i32.const 1))) (br 0 (f32.const 1.0))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-f32-vs-i64 (result f32)
    (block (result f32)
      (block (result f32) (br 1 (i64.const 1))) (br 0 (f32.const 1.0))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-f32-vs-f64 (result f32)
    (block (result f32)
      (block (result f32) (br 1 (f64.const 1.0))) (br 0 (f32.const 1.0))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-f64-vs-i32 (result f64)
    (block (result f64)
      (block (result f64) (br 1 (i32.const 1))) (br 0 (f64.const 1.0))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-f64-vs-i64 (result f64)
    (block (result f64)
      (block (result f64) (br 1 (i64.const 1))) (br 0 (f64.const 1.0))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-f64-vs-f32 (result f64)
    (block (result f64)
      (block (result f64) (br 1 (f32.const 1.0))) (br 0 (f64.const 1.0))
    )
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-num-vs-nums (result i32 i32)
    (block (result i32 i32)
      (block (result i32 i32) (br 1 (i32.const 0))) (br 0 (i32.const 1) (i32.const 2))
    )
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-operand-empty-vs-i32 (result i32)
    (i32.ctz (block (br 0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-empty-vs-i64 (result i64)
    (i64.ctz (block (br 0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-empty-vs-f32 (result f32)
    (f32.floor (block (br 0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-empty-vs-f64 (result f64)
    (f64.floor (block (br 0)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-empty-vs-nums (result i32)
    (i32.add (block (br 0)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-operand-void-vs-i32 (result i32)
    (i32.ctz (block (br 0 (nop))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-void-vs-i64 (result i64)
    (i64.ctz (block (br 0 (nop))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-void-vs-f32 (result f32)
    (f32.floor (block (br 0 (nop))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-void-vs-f64 (result f64)
    (f64.floor (block (br 0 (nop))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-void-vs-nums (result i32)
    (i32.add (block (br 0 (nop))))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-operand-i32-vs-i64 (result i32)
    (i64.ctz (block (br 0 (i64.const 9))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-i32-vs-f32 (result i32)
    (f32.floor (block (br 0 (f32.const 9.0))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-i32-vs-f64 (result i32)
    (f64.floor (block (br 0 (f64.const 9.0))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-i64-vs-i32 (result i64)
    (i32.ctz (block (br 0 (i32.const 9))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-i64-vs-f32 (result i64)
    (f32.floor (block (br 0 (f32.const 9.0))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-i64-vs-f64 (result i64)
    (f64.floor (block (br 0 (f64.const 9.0))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-f32-vs-i32 (result f32)
    (i32.ctz (block (br 0 (i32.const 9))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-f32-vs-i64 (result f32)
    (i64.ctz (block (br 0 (i64.const 9))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-f32-vs-f64 (result f32)
    (f64.floor (block (br 0 (f64.const 9.0))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-f64-vs-i32 (result f64)
    (i32.ctz (block (br 0 (i32.const 9))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-f64-vs-i64 (result f64)
    (i64.ctz (block (br 0 (i64.const 9))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-f64-vs-f32 (result f64)
    (f32.floor (block (br 0 (f32.const 9.0))))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-operand-num-vs-nums (result i32)
    (i32.add (block (br 0 (i64.const 9) (i32.const 10))))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-param-void-vs-num
    (block (param i32) (drop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-void-vs-nums
    (block (param i32 f64) (drop) (drop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-num-vs-num
    (f32.const 0) (block (param i32) (drop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-num-vs-nums
    (f32.const 0) (block (param f32 i32) (drop) (drop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-nested-void-vs-num
    (block (block (param i32) (drop)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-void-vs-nums
    (block (block (param i32 f64) (drop) (drop)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-num-vs-num
    (block (f32.const 0) (block (param i32) (drop)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-num-vs-nums
    (block (f32.const 0) (block (param f32 i32) (drop) (drop)))
  ))
  "type mismatch"
)

(assert_malformed
  (module quote "(func (param i32) (result i32) block (param $x i32) end)")
  "unexpected token"
)
(assert_malformed
  (module quote "(func (param i32) (result i32) (block (param $x i32)))")
  "unexpected token"
)


(assert_malformed
  (module quote "(func block end $l)")
  "mismatching label"
)
(assert_malformed
  (module quote "(func block $a end $l)")
  "mismatching label"
)
