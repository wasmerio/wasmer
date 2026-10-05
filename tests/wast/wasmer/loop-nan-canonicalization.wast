(module
  (type $f32_f32 (func (param f32) (result f32)))
  (type $f64_f64 (func (param f64) (result f64)))

  ;; Finite initial parameter, arithmetic-NaN backedge: Singlepass must
  ;; canonicalize the value before storing it in the loop's PHI slot.
  (func (export "f32_under") (result i32)
    f32.const 0 i32.const 0
    loop (param f32 i32) (result f32)
      if (type $f32_f32)
      else
        drop
        f32.const nan:0x400001 f32.const 1 f32.add
        i32.const 1 br 1
      end
    end
    i32.reinterpret_f32)
  (func (export "f64_under") (result i64)
    f64.const 0 i32.const 0
    loop (param f64 i32) (result f64)
      if (type $f64_f64)
      else
        drop
        f64.const nan:0x8000000000001 f64.const 1 f64.add
        i32.const 1 br 1
      end
    end
    i64.reinterpret_f64)

  ;; Arithmetic-NaN initial parameter, constant-NaN backedge: Singlepass must
  ;; not canonicalize the constant stored on the backedge.
  (func (export "f32_over") (result i32)
    f32.const nan:0x400001 f32.const 1 f32.add i32.const 0
    loop (param f32 i32) (result f32)
      if (type $f32_f32)
      else
        drop
        f32.const nan:0x400002
        i32.const 1 br 1
      end
    end
    i32.reinterpret_f32)
  (func (export "f64_over") (result i64)
    f64.const nan:0x8000000000001 f64.const 1 f64.add i32.const 0
    loop (param f64 i32) (result f64)
      if (type $f64_f64)
      else
        drop
        f64.const nan:0x8000000000002
        i32.const 1 br 1
      end
    end
    i64.reinterpret_f64)
)

(assert_return (invoke "f32_under") (i32.const 0x7fc00000))
(assert_return (invoke "f64_under") (i64.const 0x7ff8000000000000))
(assert_return (invoke "f32_over") (i32.const 0x7fc00002))
(assert_return (invoke "f64_over") (i64.const 0x7ff8000000000002))
