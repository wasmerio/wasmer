(module
  ;; Arithmetic produces an arithmetic NaN. With NaN canonicalization enabled,
  ;; reinterpretation must observe the canonical 0x7fc00000 payload even through
  ;; abs/neg (which only alter the sign bit).
  (func (export "abs") (result i32)
    f32.const nan:0x400001
    f32.const 1
    f32.add
    f32.abs
    i32.reinterpret_f32)
  (func (export "neg") (result i32)
    f32.const nan:0x400001
    f32.const 1
    f32.add
    f32.neg
    i32.reinterpret_f32)
)

(assert_return (invoke "abs") (i32.const 0x7fc00000))
(assert_return (invoke "neg") (i32.const 0xffc00000))
