(module
  ;; Pending NaN canonicalization must be applied to loop entry parameters
  ;; before their bits are reinterpreted inside the loop.
  (func (export "f32") (param f32) (result i32)
    local.get 0
    f32.sqrt
    loop (param f32) (result i32)
      i32.reinterpret_f32
    end)
  (func (export "f64") (param f64) (result i64)
    local.get 0
    f64.sqrt
    loop (param f64) (result i64)
      i64.reinterpret_f64
    end)
)

(assert_return (invoke "f32" (f32.const -1)) (i32.const 0x7fc00000))
(assert_return (invoke "f64" (f64.const -1)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f32" (f32.const 4)) (i32.const 0x40000000))
(assert_return (invoke "f64" (f64.const 4)) (i64.const 0x4000000000000000))
