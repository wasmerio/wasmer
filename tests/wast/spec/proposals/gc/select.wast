(module
  ;; Auxiliary
  (func $dummy)
  (table $tab funcref (elem $dummy))
  (memory 1)

  (func (export "select-i32") (param i32 i32 i32) (result i32)
    (select (local.get 0) (local.get 1) (local.get 2))
  )
  (func (export "select-i64") (param i64 i64 i32) (result i64)
    (select (local.get 0) (local.get 1) (local.get 2))
  )
  (func (export "select-f32") (param f32 f32 i32) (result f32)
    (select (local.get 0) (local.get 1) (local.get 2))
  )
  (func (export "select-f64") (param f64 f64 i32) (result f64)
    (select (local.get 0) (local.get 1) (local.get 2))
  )

  (func (export "select-i32-t") (param i32 i32 i32) (result i32)
    (select (result i32) (local.get 0) (local.get 1) (local.get 2))
  )
  (func (export "select-i64-t") (param i64 i64 i32) (result i64)
    (select (result i64) (local.get 0) (local.get 1) (local.get 2))
  )
  (func (export "select-f32-t") (param f32 f32 i32) (result f32)
    (select (result f32) (local.get 0) (local.get 1) (local.get 2))
  )
  (func (export "select-f64-t") (param f64 f64 i32) (result f64)
    (select (result f64) (local.get 0) (local.get 1) (local.get 2))
  )
  (func (export "select-funcref") (param funcref funcref i32) (result funcref)
    (select (result funcref) (local.get 0) (local.get 1) (local.get 2))
  )
  (func (export "select-externref") (param externref externref i32) (result externref)
    (select (result externref) (local.get 0) (local.get 1) (local.get 2))
  )

  (type $t (func))
  (func $tf) (elem declare func $tf)
  (func (export "join-funcnull") (param i32) (result (ref null func))
    (select (result (ref null func))
      (ref.func $tf)
      (ref.null func)
      (local.get 0)
    )
  )

  ;; Check that both sides of the select are evaluated
  (func (export "select-trap-left") (param $cond i32) (result i32)
    (select (unreachable) (i32.const 0) (local.get $cond))
  )
  (func (export "select-trap-right") (param $cond i32) (result i32)
    (select (i32.const 0) (unreachable) (local.get $cond))
  )

  (func (export "select-unreached")
    (unreachable) (select)
    (unreachable) (i32.const 0) (select)
    (unreachable) (i32.const 0) (i32.const 0) (select)
    (unreachable) (i32.const 0) (i32.const 0) (i32.const 0) (select)
    (unreachable) (f32.const 0) (i32.const 0) (select)
    (unreachable)
  )

  (func (export "select_unreached_result_1") (result i32)
    (unreachable) (i32.add (select))
  )

  (func (export "select_unreached_result_2") (result i64)
    (unreachable) (i64.add (select (i64.const 0) (i32.const 0)))
  )

  ;; As the argument of control constructs and instructions

  (func (export "as-select-first") (param i32) (result i32)
    (select (select (i32.const 0) (i32.const 1) (local.get 0)) (i32.const 2) (i32.const 3))
  )
  (func (export "as-select-mid") (param i32) (result i32)
    (select (i32.const 2) (select (i32.const 0) (i32.const 1) (local.get 0)) (i32.const 3))
  )
  (func (export "as-select-last") (param i32) (result i32)
    (select (i32.const 2) (i32.const 3) (select (i32.const 0) (i32.const 1) (local.get 0)))
  )

  (func (export "as-loop-first") (param i32) (result i32)
    (loop (result i32) (select (i32.const 2) (i32.const 3) (local.get 0)) (call $dummy) (call $dummy))
  )
  (func (export "as-loop-mid") (param i32) (result i32)
    (loop (result i32) (call $dummy) (select (i32.const 2) (i32.const 3) (local.get 0)) (call $dummy))
  )
  (func (export "as-loop-last") (param i32) (result i32)
    (loop (result i32) (call $dummy) (call $dummy) (select (i32.const 2) (i32.const 3) (local.get 0)))
  )

  (func (export "as-if-condition") (param i32)
    (select (i32.const 2) (i32.const 3) (local.get 0)) (if (then (call $dummy)))
  )
  (func (export "as-if-then") (param i32) (result i32)
    (if (result i32) (i32.const 1) (then (select (i32.const 2) (i32.const 3) (local.get 0))) (else (i32.const 4)))
  )
  (func (export "as-if-else") (param i32) (result i32)
    (if (result i32) (i32.const 0) (then (i32.const 2)) (else (select (i32.const 2) (i32.const 3) (local.get 0))))
  )

  (func (export "as-br_if-first") (param i32) (result i32)
    (block (result i32) (br_if 0 (select (i32.const 2) (i32.const 3) (local.get 0)) (i32.const 4)))
  )
  (func (export "as-br_if-last") (param i32) (result i32)
    (block (result i32) (br_if 0 (i32.const 2) (select (i32.const 2) (i32.const 3) (local.get 0))))
  )

  (func (export "as-br_table-first") (param i32) (result i32)
    (block (result i32) (select (i32.const 2) (i32.const 3) (local.get 0)) (i32.const 2) (br_table 0 0))
  )
  (func (export "as-br_table-last") (param i32) (result i32)
    (block (result i32) (i32.const 2) (select (i32.const 2) (i32.const 3) (local.get 0)) (br_table 0 0))
  )

  (func $func (param i32 i32) (result i32) (local.get 0))
  (type $check (func (param i32 i32) (result i32)))
  (table $t funcref (elem $func))
  (func (export "as-call_indirect-first") (param i32) (result i32)
    (block (result i32)
      (call_indirect $t (type $check)
        (select (i32.const 2) (i32.const 3) (local.get 0)) (i32.const 1) (i32.const 0)
      )
    )
  )
  (func (export "as-call_indirect-mid") (param i32) (result i32)
    (block (result i32)
      (call_indirect $t (type $check)
        (i32.const 1) (select (i32.const 2) (i32.const 3) (local.get 0)) (i32.const 0)
      )
    )
  )
  (func (export "as-call_indirect-last") (param i32) (result i32)
    (block (result i32)
      (call_indirect $t (type $check)
        (i32.const 1) (i32.const 4) (select (i32.const 2) (i32.const 3) (local.get 0))
      )
    )
  )

  (func (export "as-store-first") (param i32)
    (select (i32.const 0) (i32.const 4) (local.get 0)) (i32.const 1) (i32.store)
  )
  (func (export "as-store-last") (param i32)
    (i32.const 8) (select (i32.const 1) (i32.const 2) (local.get 0)) (i32.store)
  )

  (func (export "as-memory.grow-value") (param i32) (result i32)
    (memory.grow (select (i32.const 1) (i32.const 2) (local.get 0)))
  )

  (func $f (param i32) (result i32) (local.get 0))

  (func (export "as-call-value") (param i32) (result i32)
    (call $f (select (i32.const 1) (i32.const 2) (local.get 0)))
  )
  (func (export "as-return-value") (param i32) (result i32)
    (select (i32.const 1) (i32.const 2) (local.get 0)) (return)
  )
  (func (export "as-drop-operand") (param i32)
    (drop (select (i32.const 1) (i32.const 2) (local.get 0)))
  )
  (func (export "as-br-value") (param i32) (result i32)
    (block (result i32) (br 0 (select (i32.const 1) (i32.const 2) (local.get 0))))
  )
  (func (export "as-local.set-value") (param i32) (result i32)
    (local i32) (local.set 0 (select (i32.const 1) (i32.const 2) (local.get 0))) (local.get 0)
  )
  (func (export "as-local.tee-value") (param i32) (result i32)
    (local.tee 0 (select (i32.const 1) (i32.const 2) (local.get 0)))
  )
  (global $a (mut i32) (i32.const 10))
  (func (export "as-global.set-value") (param i32) (result i32)
    (global.set $a (select (i32.const 1) (i32.const 2) (local.get 0)))
    (global.get $a)
  )
  (func (export "as-load-operand") (param i32) (result i32)
    (i32.load (select (i32.const 0) (i32.const 4) (local.get 0)))
  )

  (func (export "as-unary-operand") (param i32) (result i32)
    (i32.eqz (select (i32.const 0) (i32.const 1) (local.get 0)))
  )
  (func (export "as-binary-operand") (param i32) (result i32)
    (i32.mul
      (select (i32.const 1) (i32.const 2) (local.get 0))
      (select (i32.const 1) (i32.const 2) (local.get 0))
    )
  )
  (func (export "as-test-operand") (param i32) (result i32)
    (block (result i32)
      (i32.eqz (select (i32.const 0) (i32.const 1) (local.get 0)))
    )
  )

  (func (export "as-compare-left") (param i32) (result i32)
    (block (result i32)
      (i32.le_s (select (i32.const 1) (i32.const 2) (local.get 0)) (i32.const 1))
    )
  )
  (func (export "as-compare-right") (param i32) (result i32)
    (block (result i32)
      (i32.ne (i32.const 1) (select (i32.const 0) (i32.const 1) (local.get 0)))
    )
  )

  (func (export "as-convert-operand") (param i32) (result i32)
    (block (result i32)
      (i32.wrap_i64 (select (i64.const 1) (i64.const 0) (local.get 0)))
    )
  )

  (func (export "unreachable-num")
    (unreachable)
    (select)
    (i32.eqz)
    (drop)
  )
  (func (export "unreachable-ref")
    (unreachable)
    (select)
    (ref.is_null)
    (drop)
  )
)

(assert_return (invoke "select-i32" (i32.const 1) (i32.const 2) (i32.const 1)) (i32.const 1))
(assert_return (invoke "select-i64" (i64.const 2) (i64.const 1) (i32.const 1)) (i64.const 2))
(assert_return (invoke "select-f32" (f32.const 1) (f32.const 2) (i32.const 1)) (f32.const 1))
(assert_return (invoke "select-f64" (f64.const 1) (f64.const 2) (i32.const 1)) (f64.const 1))

(assert_return (invoke "select-i32" (i32.const 1) (i32.const 2) (i32.const 0)) (i32.const 2))
(assert_return (invoke "select-i32" (i32.const 2) (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "select-i64" (i64.const 2) (i64.const 1) (i32.const -1)) (i64.const 2))
(assert_return (invoke "select-i64" (i64.const 2) (i64.const 1) (i32.const 0xf0f0f0f0)) (i64.const 2))

(assert_return (invoke "select-f32" (f32.const nan) (f32.const 1) (i32.const 1)) (f32.const nan))
(assert_return (invoke "select-f32" (f32.const nan:0x20304) (f32.const 1) (i32.const 1)) (f32.const nan:0x20304))
(assert_return (invoke "select-f32" (f32.const nan) (f32.const 1) (i32.const 0)) (f32.const 1))
(assert_return (invoke "select-f32" (f32.const nan:0x20304) (f32.const 1) (i32.const 0)) (f32.const 1))
(assert_return (invoke "select-f32" (f32.const 2) (f32.const nan) (i32.const 1)) (f32.const 2))
(assert_return (invoke "select-f32" (f32.const 2) (f32.const nan:0x20304) (i32.const 1)) (f32.const 2))
(assert_return (invoke "select-f32" (f32.const 2) (f32.const nan) (i32.const 0)) (f32.const nan))
(assert_return (invoke "select-f32" (f32.const 2) (f32.const nan:0x20304) (i32.const 0)) (f32.const nan:0x20304))

(assert_return (invoke "select-f64" (f64.const nan) (f64.const 1) (i32.const 1)) (f64.const nan))
(assert_return (invoke "select-f64" (f64.const nan:0x20304) (f64.const 1) (i32.const 1)) (f64.const nan:0x20304))
(assert_return (invoke "select-f64" (f64.const nan) (f64.const 1) (i32.const 0)) (f64.const 1))
(assert_return (invoke "select-f64" (f64.const nan:0x20304) (f64.const 1) (i32.const 0)) (f64.const 1))
(assert_return (invoke "select-f64" (f64.const 2) (f64.const nan) (i32.const 1)) (f64.const 2))
(assert_return (invoke "select-f64" (f64.const 2) (f64.const nan:0x20304) (i32.const 1)) (f64.const 2))
(assert_return (invoke "select-f64" (f64.const 2) (f64.const nan) (i32.const 0)) (f64.const nan))
(assert_return (invoke "select-f64" (f64.const 2) (f64.const nan:0x20304) (i32.const 0)) (f64.const nan:0x20304))

(assert_return (invoke "select-i32-t" (i32.const 1) (i32.const 2) (i32.const 1)) (i32.const 1))
(assert_return (invoke "select-i64-t" (i64.const 2) (i64.const 1) (i32.const 1)) (i64.const 2))
(assert_return (invoke "select-f32-t" (f32.const 1) (f32.const 2) (i32.const 1)) (f32.const 1))
(assert_return (invoke "select-f64-t" (f64.const 1) (f64.const 2) (i32.const 1)) (f64.const 1))
(assert_return (invoke "select-funcref" (ref.null func) (ref.null func) (i32.const 1)) (ref.null func))
(assert_return (invoke "select-externref" (ref.extern 1) (ref.extern 2) (i32.const 1)) (ref.extern 1))

(assert_return (invoke "select-i32-t" (i32.const 1) (i32.const 2) (i32.const 0)) (i32.const 2))
(assert_return (invoke "select-i32-t" (i32.const 2) (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "select-i64-t" (i64.const 2) (i64.const 1) (i32.const -1)) (i64.const 2))
(assert_return (invoke "select-i64-t" (i64.const 2) (i64.const 1) (i32.const 0xf0f0f0f0)) (i64.const 2))
(assert_return (invoke "select-externref" (ref.extern 1) (ref.extern 2) (i32.const 0)) (ref.extern 2))
(assert_return (invoke "select-externref" (ref.extern 2) (ref.extern 1) (i32.const 0)) (ref.extern 1))

(assert_return (invoke "select-f32-t" (f32.const nan) (f32.const 1) (i32.const 1)) (f32.const nan))
(assert_return (invoke "select-f32-t" (f32.const nan:0x20304) (f32.const 1) (i32.const 1)) (f32.const nan:0x20304))
(assert_return (invoke "select-f32-t" (f32.const nan) (f32.const 1) (i32.const 0)) (f32.const 1))
(assert_return (invoke "select-f32-t" (f32.const nan:0x20304) (f32.const 1) (i32.const 0)) (f32.const 1))
(assert_return (invoke "select-f32-t" (f32.const 2) (f32.const nan) (i32.const 1)) (f32.const 2))
(assert_return (invoke "select-f32-t" (f32.const 2) (f32.const nan:0x20304) (i32.const 1)) (f32.const 2))
(assert_return (invoke "select-f32-t" (f32.const 2) (f32.const nan) (i32.const 0)) (f32.const nan))
(assert_return (invoke "select-f32-t" (f32.const 2) (f32.const nan:0x20304) (i32.const 0)) (f32.const nan:0x20304))

(assert_return (invoke "select-f64-t" (f64.const nan) (f64.const 1) (i32.const 1)) (f64.const nan))
(assert_return (invoke "select-f64-t" (f64.const nan:0x20304) (f64.const 1) (i32.const 1)) (f64.const nan:0x20304))
(assert_return (invoke "select-f64-t" (f64.const nan) (f64.const 1) (i32.const 0)) (f64.const 1))
(assert_return (invoke "select-f64-t" (f64.const nan:0x20304) (f64.const 1) (i32.const 0)) (f64.const 1))
(assert_return (invoke "select-f64-t" (f64.const 2) (f64.const nan) (i32.const 1)) (f64.const 2))
(assert_return (invoke "select-f64-t" (f64.const 2) (f64.const nan:0x20304) (i32.const 1)) (f64.const 2))
(assert_return (invoke "select-f64-t" (f64.const 2) (f64.const nan) (i32.const 0)) (f64.const nan))
(assert_return (invoke "select-f64-t" (f64.const 2) (f64.const nan:0x20304) (i32.const 0)) (f64.const nan:0x20304))

(assert_return (invoke "join-funcnull" (i32.const 1)) (ref.func))
(assert_return (invoke "join-funcnull" (i32.const 0)) (ref.null))

(assert_trap (invoke "select-trap-left" (i32.const 1)) "unreachable")
(assert_trap (invoke "select-trap-left" (i32.const 0)) "unreachable")
(assert_trap (invoke "select-trap-right" (i32.const 1)) "unreachable")
(assert_trap (invoke "select-trap-right" (i32.const 0)) "unreachable")

(assert_return (invoke "as-select-first" (i32.const 0)) (i32.const 1))
(assert_return (invoke "as-select-first" (i32.const 1)) (i32.const 0))
(assert_return (invoke "as-select-mid" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-select-mid" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-select-last" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-select-last" (i32.const 1)) (i32.const 3))

(assert_return (invoke "as-loop-first" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-loop-first" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-loop-mid" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-loop-mid" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-loop-last" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-loop-last" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-if-condition" (i32.const 0)))
(assert_return (invoke "as-if-condition" (i32.const 1)))
(assert_return (invoke "as-if-then" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-if-then" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-if-else" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-if-else" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-br_if-first" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-br_if-first" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-br_if-last" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-br_if-last" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-br_table-first" (i32.const 0)) (i32.const 3))
(assert_return (invoke "as-br_table-first" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-br_table-last" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-br_table-last" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-call_indirect-first" (i32.const 0)) (i32.const 3))
;;(assert_return (invoke "as-call_indirect-first" (i32.const 1)) (i32.const 2))
(assert_return (invoke "as-call_indirect-mid" (i32.const 0)) (i32.const 1))
(assert_return (invoke "as-call_indirect-mid" (i32.const 1)) (i32.const 1))
(assert_trap (invoke "as-call_indirect-last" (i32.const 0)) "undefined element")
(assert_trap (invoke "as-call_indirect-last" (i32.const 1)) "undefined element")

(assert_return (invoke "as-store-first" (i32.const 0)))
(assert_return (invoke "as-store-first" (i32.const 1)))
(assert_return (invoke "as-store-last" (i32.const 0)))
(assert_return (invoke "as-store-last" (i32.const 1)))

(assert_return (invoke "as-memory.grow-value" (i32.const 0)) (i32.const 1))
(assert_return (invoke "as-memory.grow-value" (i32.const 1)) (i32.const 3))

(assert_return (invoke "as-call-value" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-call-value" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-return-value" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-return-value" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-drop-operand" (i32.const 0)))
(assert_return (invoke "as-drop-operand" (i32.const 1)))
(assert_return (invoke "as-br-value" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-br-value" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-local.set-value" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-local.set-value" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-local.tee-value" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-local.tee-value" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-global.set-value" (i32.const 0)) (i32.const 2))
(assert_return (invoke "as-global.set-value" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-load-operand" (i32.const 0)) (i32.const 1))
(assert_return (invoke "as-load-operand" (i32.const 1)) (i32.const 1))

(assert_return (invoke "as-unary-operand" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-unary-operand" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-binary-operand" (i32.const 0)) (i32.const 4))
(assert_return (invoke "as-binary-operand" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-test-operand" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-test-operand" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-compare-left" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-compare-left" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-compare-right" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-compare-right" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-convert-operand" (i32.const 0)) (i32.const 0))
(assert_return (invoke "as-convert-operand" (i32.const 1)) (i32.const 1))

(assert_invalid
  (module (func $arity-0-implicit (select (nop) (nop) (i32.const 1))))
  "type mismatch"
)
(assert_invalid
  (module (func $arity-0 (select (result) (nop) (nop) (i32.const 1))))
  "invalid result arity"
)
(assert_invalid
  (module (func $arity-2 (result i32 i32)
    (select (result i32 i32)
      (i32.const 0) (i32.const 0)
      (i32.const 0) (i32.const 0)
      (i32.const 1)
    )
  ))
  "invalid result arity"
)


(assert_invalid
  (module (type $t (func))
    (func $type-ref-implicit (param $r (ref $t))
      (drop (select (local.get $r) (local.get $r) (i32.const 1)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module (func $type-funcref-implicit (param $r funcref)
    (drop (select (local.get $r) (local.get $r) (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-externref-implicit (param $r externref)
    (drop (select (local.get $r) (local.get $r) (i32.const 1)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-num-vs-num
    (drop (select (i32.const 1) (i64.const 1) (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-num-vs-num
    (drop (select (i32.const 1) (f32.const 1.0) (i32.const 1)))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-num-vs-num
    (drop (select (i32.const 1) (f64.const 1.0) (i32.const 1)))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-num-vs-num (select (i32.const 1) (i64.const 1) (i32.const 1)) (drop)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-num-vs-num (select (i32.const 1) (f32.const 1.0) (i32.const 1)) (drop)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-num-vs-num (select (i32.const 1) (i64.const 1) (i32.const 1)) (drop)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-num-vs-num (select (i32.const 1) (f32.const 1.0) (i32.const 1)) (drop)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-num-vs-num (select (i32.const 1) (f64.const 1.0) (i32.const 1)) (drop)))
  "type mismatch"
)


(assert_invalid
  (module
    (func $type-1st-operand-empty
      (select) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-2nd-operand-empty
      (i32.const 0) (select) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-3rd-operand-empty
      (i32.const 0) (i32.const 0) (select) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-1st-operand-empty-in-block
      (i32.const 0) (i32.const 0) (i32.const 0)
      (block (select) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-2nd-operand-empty-in-block
      (i32.const 0) (i32.const 0)
      (block (i32.const 0) (select) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-3rd-operand-empty-in-block
      (i32.const 0)
      (block (i32.const 0) (i32.const 0) (select) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-1st-operand-empty-in-loop
      (i32.const 0) (i32.const 0) (i32.const 0)
      (loop (select) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-2nd-operand-empty-in-loop
      (i32.const 0) (i32.const 0)
      (loop (i32.const 0) (select) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-3rd-operand-empty-in-loop
      (i32.const 0)
      (loop (i32.const 0) (i32.const 0) (select) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-1st-operand-empty-in-then
      (i32.const 0) (i32.const 0) (i32.const 0)
      (if (then (select) (drop)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-2nd-operand-empty-in-then
      (i32.const 0) (i32.const 0)
      (if (then (i32.const 0) (select) (drop)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-3rd-operand-empty-in-then
      (i32.const 0)
      (if (then (i32.const 0) (i32.const 0) (select) (drop)))
    )
  )
  "type mismatch"
)

;; Third operand must be i32

(assert_invalid
  (module (func (select (i32.const 1) (i32.const 1) (i64.const 1)) (drop)))
  "type mismatch"
)
(assert_invalid
  (module (func (select (i32.const 1) (i32.const 1) (f32.const 1)) (drop)))
  "type mismatch"
)
(assert_invalid
  (module (func (select (i32.const 1) (i32.const 1) (f64.const 1)) (drop)))
  "type mismatch"
)

;; Result of select has type of first two operands

(assert_invalid
  (module (func (result i32) (select (i64.const 1) (i64.const 1) (i32.const 1))))
  "type mismatch"
)

;; Validation after unreachable

;; The first two operands should have the same type as each other
(assert_invalid
  (module (func (unreachable) (select (i32.const 1) (i64.const 1) (i32.const 1)) (drop)))
  "type mismatch"
)

(assert_invalid
  (module (func (unreachable) (select (i64.const 1) (i32.const 1) (i32.const 1)) (drop)))
  "type mismatch"
)

;; Third operand must be i32
(assert_invalid
  (module (func (unreachable) (select (i32.const 1) (i32.const 1) (i64.const 1)) (drop)))
  "type mismatch"
)

(assert_invalid
  (module (func (unreachable) (select (i32.const 1) (i64.const 1)) (drop)))
  "type mismatch"
)

(assert_invalid
  (module (func (unreachable) (select (i64.const 1)) (drop)))
  "type mismatch"
)

;; Result of select has type of first two operands (type of second operand when first one is omitted)
(assert_invalid
  (module (func (result i32) (unreachable) (select (i64.const 1) (i32.const 1))))
  "type mismatch"
)

;; select always has non-empty result
(assert_invalid
  (module (func (unreachable) (select)))
  "type mismatch"
)

