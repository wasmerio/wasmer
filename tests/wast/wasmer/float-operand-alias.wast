;; Exercise SSE's destructive operands with both aliased and distinct results.
(module
  ;; The right operand's value register is reused for the result.
  (func (export "f32-sub") (param f32) (result f32)
    f32.const 12 local.get 0 f32.const 1 f32.mul f32.sub)
  (func (export "f32-div") (param f32) (result f32)
    f32.const 12 local.get 0 f32.const 1 f32.mul f32.div)
  (func (export "f64-sub") (param f64) (result f64)
    f64.const 12 local.get 0 f64.const 1 f64.mul f64.sub)
  (func (export "f64-div") (param f64) (result f64)
    f64.const 12 local.get 0 f64.const 1 f64.mul f64.div)

  ;; Local inputs and results occupy different locations.
  (func (export "f32-unary") (param f32) (result f32 f32 f32 f32 f32)
    local.get 0 f32.sqrt
    local.get 0 f32.trunc
    local.get 0 f32.ceil
    local.get 0 f32.floor
    local.get 0 f32.nearest)
  (func (export "f64-unary") (param f64) (result f64 f64 f64 f64 f64)
    local.get 0 f64.sqrt
    local.get 0 f64.trunc
    local.get 0 f64.ceil
    local.get 0 f64.floor
    local.get 0 f64.nearest)
  (func (export "promote") (param f32) (result f64)
    local.get 0 f64.promote_f32)
  (func (export "demote") (param f64) (result f32)
    local.get 0 f32.demote_f64)
  (func (export "convert") (param i32) (result f32 f64)
    local.get 0 f32.convert_i32_s
    local.get 0 f64.convert_i32_s)

  (func (export "f32-compare") (param f32 f32) (result i32 i32 i32 i32)
    local.get 0 local.get 1 f32.lt
    local.get 0 local.get 1 f32.le
    local.get 0 local.get 1 f32.gt
    local.get 0 local.get 1 f32.ge)
  (func (export "f64-compare") (param f64 f64) (result i32 i32 i32 i32)
    local.get 0 local.get 1 f64.lt
    local.get 0 local.get 1 f64.le
    local.get 0 local.get 1 f64.gt
    local.get 0 local.get 1 f64.ge)

  ;; Keep the value-register pool full so comparison operands spill to memory.
  (func (export "f32-compare-spill") (param f32 f32) (result i32 i32)
    (local $gt i32) (local $ge i32)
    f32.const 1 f32.sqrt
    f32.const 1 f32.sqrt
    f32.const 1 f32.sqrt
    f32.const 1 f32.sqrt
    f32.const 1 f32.sqrt
    local.get 0 f32.const 0 f32.add
    local.get 1 f32.const 0 f32.add
    f32.gt local.set $gt
    local.get 0 f32.const 0 f32.add
    local.get 1 f32.const 0 f32.add
    f32.ge local.set $ge
    drop drop drop drop drop
    local.get $gt local.get $ge)
  (func (export "f64-compare-spill") (param f64 f64) (result i32 i32)
    (local $gt i32) (local $ge i32)
    f64.const 1 f64.sqrt
    f64.const 1 f64.sqrt
    f64.const 1 f64.sqrt
    f64.const 1 f64.sqrt
    f64.const 1 f64.sqrt
    local.get 0 f64.const 0 f64.add
    local.get 1 f64.const 0 f64.add
    f64.gt local.set $gt
    local.get 0 f64.const 0 f64.add
    local.get 1 f64.const 0 f64.add
    f64.ge local.set $ge
    drop drop drop drop drop
    local.get $gt local.get $ge)
)

(assert_return (invoke "f32-sub" (f32.const 3)) (f32.const 9))
(assert_return (invoke "f32-div" (f32.const 3)) (f32.const 4))
(assert_return (invoke "f64-sub" (f64.const 3)) (f64.const 9))
(assert_return (invoke "f64-div" (f64.const 3)) (f64.const 4))
(assert_return (invoke "f32-unary" (f32.const 2.25))
  (f32.const 1.5) (f32.const 2) (f32.const 3) (f32.const 2) (f32.const 2))
(assert_return (invoke "f64-unary" (f64.const 2.25))
  (f64.const 1.5) (f64.const 2) (f64.const 3) (f64.const 2) (f64.const 2))
(assert_return (invoke "promote" (f32.const 1.5)) (f64.const 1.5))
(assert_return (invoke "demote" (f64.const 1.5)) (f32.const 1.5))
(assert_return (invoke "convert" (i32.const -7)) (f32.const -7) (f64.const -7))

(assert_return (invoke "f32-compare" (f32.const 2) (f32.const 3))
  (i32.const 1) (i32.const 1) (i32.const 0) (i32.const 0))
(assert_return (invoke "f32-compare" (f32.const 3) (f32.const 2))
  (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 1))
(assert_return (invoke "f32-compare" (f32.const 2) (f32.const 2))
  (i32.const 0) (i32.const 1) (i32.const 0) (i32.const 1))
(assert_return (invoke "f32-compare" (f32.const nan) (f32.const 2))
  (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))
(assert_return (invoke "f32-compare" (f32.const 2) (f32.const nan))
  (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))
(assert_return (invoke "f64-compare" (f64.const 2) (f64.const 3))
  (i32.const 1) (i32.const 1) (i32.const 0) (i32.const 0))
(assert_return (invoke "f64-compare" (f64.const 3) (f64.const 2))
  (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 1))
(assert_return (invoke "f64-compare" (f64.const 2) (f64.const 2))
  (i32.const 0) (i32.const 1) (i32.const 0) (i32.const 1))
(assert_return (invoke "f64-compare" (f64.const nan) (f64.const 2))
  (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))
(assert_return (invoke "f64-compare" (f64.const 2) (f64.const nan))
  (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0))

(assert_return (invoke "f32-compare-spill" (f32.const 3) (f32.const 2))
  (i32.const 1) (i32.const 1))
(assert_return (invoke "f32-compare-spill" (f32.const 2) (f32.const 2))
  (i32.const 0) (i32.const 1))
(assert_return (invoke "f32-compare-spill" (f32.const 2) (f32.const 3))
  (i32.const 0) (i32.const 0))
(assert_return (invoke "f32-compare-spill" (f32.const nan) (f32.const 2))
  (i32.const 0) (i32.const 0))
(assert_return (invoke "f64-compare-spill" (f64.const 3) (f64.const 2))
  (i32.const 1) (i32.const 1))
(assert_return (invoke "f64-compare-spill" (f64.const 2) (f64.const 2))
  (i32.const 0) (i32.const 1))
(assert_return (invoke "f64-compare-spill" (f64.const 2) (f64.const 3))
  (i32.const 0) (i32.const 0))
(assert_return (invoke "f64-compare-spill" (f64.const 2) (f64.const nan))
  (i32.const 0) (i32.const 0))
