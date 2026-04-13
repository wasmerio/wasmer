(module
  (memory 1)

  (func (export "unaligned-i32-load") (result i32)
    (i32.store (i32.const 0x1) (i32.const 0x78563412))
    (i32.load (i32.const 0x1))
  )

  (func (export "unaligned-i64-load") (result i64)
    (i64.store (i32.const 0x1) (i64.const 0x0123456789abcdef))
    (i64.load (i32.const 0x1))
  )

  (func (export "unaligned-f32-load-bits") (result i32)
    (f32.store (i32.const 0x1) (f32.reinterpret_i32 (i32.const 0x41200000)))
    (i32.reinterpret_f32 (f32.load (i32.const 0x1)))
  )

  (func (export "unaligned-f64-load-bits") (result i64)
    (f64.store (i32.const 0x1) (f64.reinterpret_i64 (i64.const 0x4028cccccccccccd)))
    (i64.reinterpret_f64 (f64.load (i32.const 0x1)))
  )

  (func (export "unaligned-i32-load16-u") (result i32)
    (i32.store16 (i32.const 0x1) (i32.const 0xff80))
    (i32.load16_u (i32.const 0x1))
  )

  (func (export "unaligned-i32-load16-s") (result i32)
    (i32.store16 (i32.const 0x1) (i32.const 0xff80))
    (i32.load16_s (i32.const 0x1))
  )

  (func (export "unaligned-i64-load16-u") (result i64)
    (i64.store16 (i32.const 0x1) (i64.const 0xff80))
    (i64.load16_u (i32.const 0x1))
  )

  (func (export "unaligned-i64-load16-s") (result i64)
    (i64.store16 (i32.const 0x1) (i64.const 0xff80))
    (i64.load16_s (i32.const 0x1))
  )

  (func (export "unaligned-i64-load32-u") (result i64)
    (i64.store32 (i32.const 0x1) (i64.const 0x89abcdef))
    (i64.load32_u (i32.const 0x1))
  )

  (func (export "unaligned-i64-load32-s") (result i64)
    (i64.store32 (i32.const 0x1) (i64.const 0x89abcdef))
    (i64.load32_s (i32.const 0x1))
  )
)

(assert_return (invoke "unaligned-i32-load") (i32.const 0x78563412))
(assert_return (invoke "unaligned-i64-load") (i64.const 0x0123456789abcdef))
(assert_return (invoke "unaligned-f32-load-bits") (i32.const 0x41200000))
(assert_return (invoke "unaligned-f64-load-bits") (i64.const 0x4028cccccccccccd))
(assert_return (invoke "unaligned-i32-load16-u") (i32.const 65408))
(assert_return (invoke "unaligned-i32-load16-s") (i32.const -128))
(assert_return (invoke "unaligned-i64-load16-u") (i64.const 65408))
(assert_return (invoke "unaligned-i64-load16-s") (i64.const -128))
(assert_return (invoke "unaligned-i64-load32-u") (i64.const 2309737967))
(assert_return (invoke "unaligned-i64-load32-s") (i64.const -1985229329))
