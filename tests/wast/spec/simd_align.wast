;; Valid alignment

(module (memory 1) (func (drop (v128.load align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load align=8 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load align=16 (i32.const 0)))))

(module (memory 1) (func (v128.store align=1 (i32.const 0) (v128.const i32x4 0 1 2 3))))
(module (memory 1) (func (v128.store align=2 (i32.const 0) (v128.const i32x4 0 1 2 3))))
(module (memory 1) (func (v128.store align=4 (i32.const 0) (v128.const i32x4 0 1 2 3))))
(module (memory 1) (func (v128.store align=8 (i32.const 0) (v128.const i32x4 0 1 2 3))))
(module (memory 1) (func (v128.store align=16 (i32.const 0) (v128.const i32x4 0 1 2 3))))

(module (memory 1) (func (drop (v128.load8x8_s align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load8x8_s align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load8x8_s align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load8x8_s align=8 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load8x8_u align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load8x8_u align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load8x8_u align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load8x8_u align=8 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16x4_s align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16x4_s align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16x4_s align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16x4_s align=8 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16x4_u align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16x4_u align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16x4_u align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16x4_u align=8 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32x2_s align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32x2_s align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32x2_s align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32x2_s align=8 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32x2_u align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32x2_u align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32x2_u align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32x2_u align=8 (i32.const 0)))))

(module (memory 1) (func (drop (v128.load8_splat align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16_splat align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load16_splat align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32_splat align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32_splat align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load32_splat align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load64_splat align=1 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load64_splat align=2 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load64_splat align=4 (i32.const 0)))))
(module (memory 1) (func (drop (v128.load64_splat align=8 (i32.const 0)))))

;; Invalid alignment

(assert_invalid
  (module (memory 1) (func (drop (v128.load align=32 (i32.const 0)))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 0) (func(v128.store align=32 (i32.const 0) (v128.const i32x4 0 0 0 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load8x8_s align=16 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load8x8_u align=16 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load16x4_s align=16 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load16x4_u align=16 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load32x2_s align=16 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load32x2_u align=16 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load8_splat align=2 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load16_splat align=4 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load32_splat align=8 (i32.const 0))))
  "alignment must not be larger than natural"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.load64_splat align=16 (i32.const 0))))
  "alignment must not be larger than natural"
)

;; Malformed alignment

(assert_malformed
  (module quote
    "(memory 1) (func (drop (v128.load align=-1 (i32.const 0))))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (drop (v128.load align=0 (i32.const 0))))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (drop (v128.load align=7 (i32.const 0))))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (v128.store align=-1 (i32.const 0) (v128.const i32x4 0 0 0 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 0) (func (v128.store align=0 (i32.const 0) (v128.const i32x4 0 0 0 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 0) (func (v128.store align=7 (i32.const 0) (v128.const i32x4 0 0 0 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load8x8_s align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load8x8_s align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load8x8_s align=7 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load8x8_u align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load8x8_u align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load8x8_u align=7 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load16x4_s align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load16x4_s align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load16x4_s align=7 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load16x4_u align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load16x4_u align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load16x4_u align=7 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32x2_s align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32x2_s align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32x2_s align=7 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32x2_u align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32x2_u align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32x2_u align=7 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load8_splat align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load8_splat align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load16_splat align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load16_splat align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32_splat align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32_splat align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load32_splat align=3 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load64_splat align=-1 (i32.const 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load64_splat align=0 (i32.const 0)))"
  )
  "alignment must be a power of two"
)
(assert_malformed
  (module quote
    "(memory 1) (func (result v128) (v128.load64_splat align=7 (i32.const 0)))"
  )
  "alignment must be a power of two"
)

;; Test that misaligned SIMD loads/stores don't trap

(module
  (memory 1 1)
  (func (export "v128.load align=16") (param $address i32) (result v128)
    (v128.load align=16 (local.get $address))
  )
  (func (export "v128.store align=16") (param $address i32) (param $value v128)
    (v128.store align=16 (local.get $address) (local.get $value))
  )
)

(assert_return (invoke "v128.load align=16" (i32.const 0)) (v128.const i32x4 0 0 0 0))
(assert_return (invoke "v128.load align=16" (i32.const 1)) (v128.const i32x4 0 0 0 0))
(assert_return (invoke "v128.store align=16" (i32.const 1) (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)))
(assert_return (invoke "v128.load align=16" (i32.const 0)) (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15))

;; Test aligned and unaligned read/write

(module
  (memory 1)
  (func (export "v128_unaligned_read_and_write") (result v128)
    (local v128)
    (v128.store (i32.const 0) (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15))
    (v128.load (i32.const 0))
  )
  (func (export "v128_aligned_read_and_write") (result v128)
    (local v128)
    (v128.store align=2 (i32.const 0) (v128.const i16x8 0 1 2 3 4 5 6 7))
    (v128.load align=2  (i32.const 0))
  )
  (func (export "v128_aligned_read_and_unaligned_write") (result v128)
    (local v128)
    (v128.store (i32.const 0) (v128.const i32x4 0 1 2 3))
    (v128.load align=2 (i32.const 0))
  )
  (func (export "v128_unaligned_read_and_aligned_write") (result v128)
    (local v128)
    (v128.store align=2 (i32.const 0) (v128.const i32x4 0 1 2 3))
    (v128.load (i32.const 0))
  )
)

(assert_return (invoke "v128_unaligned_read_and_write") (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15))
(assert_return (invoke "v128_aligned_read_and_write") (v128.const i16x8 0 1 2 3 4 5 6 7))
(assert_return (invoke "v128_aligned_read_and_unaligned_write") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "v128_unaligned_read_and_aligned_write") (v128.const i32x4 0 1 2 3))
