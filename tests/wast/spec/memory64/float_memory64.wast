;; Test that floating-point load and store are bit-preserving.

;; Test that load and store do not canonicalize NaNs as x87 does.

(module
  (memory i64 (data "\00\00\a0\7f"))

  (func (export "f32.load") (result f32) (f32.load (i64.const 0)))
  (func (export "i32.load") (result i32) (i32.load (i64.const 0)))
  (func (export "f32.store") (f32.store (i64.const 0) (f32.const nan:0x200000)))
  (func (export "i32.store") (i32.store (i64.const 0) (i32.const 0x7fa00000)))
  (func (export "reset") (i32.store (i64.const 0) (i32.const 0)))
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
  (memory i64 (data "\00\00\00\00\00\00\f4\7f"))

  (func (export "f64.load") (result f64) (f64.load (i64.const 0)))
  (func (export "i64.load") (result i64) (i64.load (i64.const 0)))
  (func (export "f64.store") (f64.store (i64.const 0) (f64.const nan:0x4000000000000)))
  (func (export "i64.store") (i64.store (i64.const 0) (i64.const 0x7ff4000000000000)))
  (func (export "reset") (i64.store (i64.const 0) (i64.const 0)))
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

;; Test that unaligned load and store do not canonicalize NaNs.

(module
  (memory i64 (data "\00\00\00\a0\7f"))

  (func (export "f32.load") (result f32) (f32.load (i64.const 1)))
  (func (export "i32.load") (result i32) (i32.load (i64.const 1)))
  (func (export "f32.store") (f32.store (i64.const 1) (f32.const nan:0x200000)))
  (func (export "i32.store") (i32.store (i64.const 1) (i32.const 0x7fa00000)))
  (func (export "reset") (i32.store (i64.const 1) (i32.const 0)))
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
  (memory i64 (data "\00\00\00\00\00\00\00\f4\7f"))

  (func (export "f64.load") (result f64) (f64.load (i64.const 1)))
  (func (export "i64.load") (result i64) (i64.load (i64.const 1)))
  (func (export "f64.store") (f64.store (i64.const 1) (f64.const nan:0x4000000000000)))
  (func (export "i64.store") (i64.store (i64.const 1) (i64.const 0x7ff4000000000000)))
  (func (export "reset") (i64.store (i64.const 1) (i64.const 0)))
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

;; Test that load and store do not canonicalize NaNs as some JS engines do.

(module
  (memory i64 (data "\01\00\d0\7f"))

  (func (export "f32.load") (result f32) (f32.load (i64.const 0)))
  (func (export "i32.load") (result i32) (i32.load (i64.const 0)))
  (func (export "f32.store") (f32.store (i64.const 0) (f32.const nan:0x500001)))
  (func (export "i32.store") (i32.store (i64.const 0) (i32.const 0x7fd00001)))
  (func (export "reset") (i32.store (i64.const 0) (i32.const 0)))
)

(assert_return (invoke "i32.load") (i32.const 0x7fd00001))
(assert_return (invoke "f32.load") (f32.const nan:0x500001))
(invoke "reset")
(assert_return (invoke "i32.load") (i32.const 0x0))
(assert_return (invoke "f32.load") (f32.const 0.0))
(invoke "f32.store")
(assert_return (invoke "i32.load") (i32.const 0x7fd00001))
(assert_return (invoke "f32.load") (f32.const nan:0x500001))
(invoke "reset")
(assert_return (invoke "i32.load") (i32.const 0x0))
(assert_return (invoke "f32.load") (f32.const 0.0))
(invoke "i32.store")
(assert_return (invoke "i32.load") (i32.const 0x7fd00001))
(assert_return (invoke "f32.load") (f32.const nan:0x500001))

(module
  (memory i64 (data "\01\00\00\00\00\00\fc\7f"))

  (func (export "f64.load") (result f64) (f64.load (i64.const 0)))
  (func (export "i64.load") (result i64) (i64.load (i64.const 0)))
  (func (export "f64.store") (f64.store (i64.const 0) (f64.const nan:0xc000000000001)))
  (func (export "i64.store") (i64.store (i64.const 0) (i64.const 0x7ffc000000000001)))
  (func (export "reset") (i64.store (i64.const 0) (i64.const 0)))
)

(assert_return (invoke "i64.load") (i64.const 0x7ffc000000000001))
(assert_return (invoke "f64.load") (f64.const nan:0xc000000000001))
(invoke "reset")
(assert_return (invoke "i64.load") (i64.const 0x0))
(assert_return (invoke "f64.load") (f64.const 0.0))
(invoke "f64.store")
(assert_return (invoke "i64.load") (i64.const 0x7ffc000000000001))
(assert_return (invoke "f64.load") (f64.const nan:0xc000000000001))
(invoke "reset")
(assert_return (invoke "i64.load") (i64.const 0x0))
(assert_return (invoke "f64.load") (f64.const 0.0))
(invoke "i64.store")
(assert_return (invoke "i64.load") (i64.const 0x7ffc000000000001))
(assert_return (invoke "f64.load") (f64.const nan:0xc000000000001))
