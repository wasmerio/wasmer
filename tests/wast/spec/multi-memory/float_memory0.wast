;; Test that floating-point load and store are bit-preserving.

;; Test that load and store do not canonicalize NaNs as x87 does.

(module
  (memory 0 0)
  (memory 0 0)
  (memory 0 0)
  (memory $m (data "\00\00\a0\7f"))
  (memory 0 0)
  (memory 0 0)

  (func (export "f32.load") (result f32) (f32.load $m (i32.const 0)))
  (func (export "i32.load") (result i32) (i32.load $m (i32.const 0)))
  (func (export "f32.store") (f32.store $m (i32.const 0) (f32.const nan:0x200000)))
  (func (export "i32.store") (i32.store $m (i32.const 0) (i32.const 0x7fa00000)))
  (func (export "reset") (i32.store $m (i32.const 0) (i32.const 0)))
)

(assert_return (invoke "i32.load") (i32.const 0x7fa00000))
(assert_return (invoke "f32.load") (f32.const nan:0x200000))
(invoke "reset")
(assert_return (invoke "i32.load") (i32.const 0x0))
(assert_return (invoke "f32.load") (f32.const 0.0))
(invoke "f32.store")
(assert_return (invoke "i32.load") (i32.const 0x7fa00000))
(assert_return (invoke "f32.load") (f32.const nan:0x200000))
(invoke "reset")
(assert_return (invoke "i32.load") (i32.const 0x0))
(assert_return (invoke "f32.load") (f32.const 0.0))
(invoke "i32.store")
(assert_return (invoke "i32.load") (i32.const 0x7fa00000))
(assert_return (invoke "f32.load") (f32.const nan:0x200000))

(module
  (memory 0 0)
  (memory $m (data "\00\00\00\00\00\00\f4\7f"))

  (func (export "f64.load") (result f64) (f64.load $m (i32.const 0)))
  (func (export "i64.load") (result i64) (i64.load $m (i32.const 0)))
  (func (export "f64.store") (f64.store $m (i32.const 0) (f64.const nan:0x4000000000000)))
  (func (export "i64.store") (i64.store $m (i32.const 0) (i64.const 0x7ff4000000000000)))
  (func (export "reset") (i64.store $m (i32.const 0) (i64.const 0)))
)

(assert_return (invoke "i64.load") (i64.const 0x7ff4000000000000))
(assert_return (invoke "f64.load") (f64.const nan:0x4000000000000))
(invoke "reset")
(assert_return (invoke "i64.load") (i64.const 0x0))
(assert_return (invoke "f64.load") (f64.const 0.0))
(invoke "f64.store")
(assert_return (invoke "i64.load") (i64.const 0x7ff4000000000000))
(assert_return (invoke "f64.load") (f64.const nan:0x4000000000000))
(invoke "reset")
(assert_return (invoke "i64.load") (i64.const 0x0))
(assert_return (invoke "f64.load") (f64.const 0.0))
(invoke "i64.store")
(assert_return (invoke "i64.load") (i64.const 0x7ff4000000000000))
(assert_return (invoke "f64.load") (f64.const nan:0x4000000000000))

