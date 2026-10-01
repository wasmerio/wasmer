;; Pending SIMD NaN canonicalization must be applied before reinterpreting the
;; vector with a different floating-point lane type.
(module
  (func
    v128.const f32x4 nan:0x100000 nan:0x200000 nan:0x300000 nan:0x400000
    f32x4.trunc
    f64x2.ceil
    drop
  )
  (func
    v128.const f64x2 nan:0x1000000000000 nan:0x2000000000000
    f64x2.trunc
    f32x4.ceil
    drop
  )
)
