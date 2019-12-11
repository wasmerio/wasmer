;; Test floating-point literal parsing.

(module
  ;; f32 special values
  (func (export "f32.nan") (result i32) (i32.reinterpret_f32 (f32.const nan)))
  (func (export "f32.positive_nan") (result i32) (i32.reinterpret_f32 (f32.const +nan)))
  (func (export "f32.negative_nan") (result i32) (i32.reinterpret_f32 (f32.const -nan)))
  (func (export "f32.plain_nan") (result i32) (i32.reinterpret_f32 (f32.const nan:0x400000)))
  (func (export "f32.informally_known_as_plain_snan") (result i32) (i32.reinterpret_f32 (f32.const nan:0x200000)))
  (func (export "f32.all_ones_nan") (result i32) (i32.reinterpret_f32 (f32.const -nan:0x7fffff)))
  (func (export "f32.misc_nan") (result i32) (i32.reinterpret_f32 (f32.const nan:0x012345)))
  (func (export "f32.misc_positive_nan") (result i32) (i32.reinterpret_f32 (f32.const +nan:0x304050)))
  (func (export "f32.misc_negative_nan") (result i32) (i32.reinterpret_f32 (f32.const -nan:0x2abcde)))
  (func (export "f32.infinity") (result i32) (i32.reinterpret_f32 (f32.const inf)))
  (func (export "f32.positive_infinity") (result i32) (i32.reinterpret_f32 (f32.const +inf)))
  (func (export "f32.negative_infinity") (result i32) (i32.reinterpret_f32 (f32.const -inf)))

  ;; f32 numbers
  (func (export "f32.zero") (result i32) (i32.reinterpret_f32 (f32.const 0x0.0p0)))
  (func (export "f32.positive_zero") (result i32) (i32.reinterpret_f32 (f32.const +0x0.0p0)))
  (func (export "f32.negative_zero") (result i32) (i32.reinterpret_f32 (f32.const -0x0.0p0)))
  (func (export "f32.misc") (result i32) (i32.reinterpret_f32 (f32.const 0x1.921fb6p+2)))
  (func (export "f32.min_positive") (result i32) (i32.reinterpret_f32 (f32.const 0x1p-149)))
  (func (export "f32.min_normal") (result i32) (i32.reinterpret_f32 (f32.const 0x1p-126)))
  (func (export "f32.max_finite") (result i32) (i32.reinterpret_f32 (f32.const 0x1.fffffep+127)))
  (func (export "f32.max_subnormal") (result i32) (i32.reinterpret_f32 (f32.const 0x1.fffffcp-127)))
  (func (export "f32.trailing_dot") (result i32) (i32.reinterpret_f32 (f32.const 0x1.p10)))

  ;; f32 in decimal format
  (func (export "f32_dec.zero") (result i32) (i32.reinterpret_f32 (f32.const 0.0e0)))
  (func (export "f32_dec.positive_zero") (result i32) (i32.reinterpret_f32 (f32.const +0.0e0)))
  (func (export "f32_dec.negative_zero") (result i32) (i32.reinterpret_f32 (f32.const -0.0e0)))
  (func (export "f32_dec.misc") (result i32) (i32.reinterpret_f32 (f32.const 6.28318548202514648)))
  (func (export "f32_dec.min_positive") (result i32) (i32.reinterpret_f32 (f32.const 1.4013e-45)))
  (func (export "f32_dec.min_normal") (result i32) (i32.reinterpret_f32 (f32.const 1.1754944e-38)))
  (func (export "f32_dec.max_subnormal") (result i32) (i32.reinterpret_f32 (f32.const 1.1754942e-38)))
  (func (export "f32_dec.max_finite") (result i32) (i32.reinterpret_f32 (f32.const 3.4028234e+38)))
  (func (export "f32_dec.trailing_dot") (result i32) (i32.reinterpret_f32 (f32.const 1.e10)))

  ;; https://twitter.com/Archivd/status/994637336506912768
  (func (export "f32_dec.root_beer_float") (result i32) (i32.reinterpret_f32 (f32.const 1.000000119)))

  ;; f64 special values
  (func (export "f64.nan") (result i64) (i64.reinterpret_f64 (f64.const nan)))
  (func (export "f64.positive_nan") (result i64) (i64.reinterpret_f64 (f64.const +nan)))
  (func (export "f64.negative_nan") (result i64) (i64.reinterpret_f64 (f64.const -nan)))
  (func (export "f64.plain_nan") (result i64) (i64.reinterpret_f64 (f64.const nan:0x8000000000000)))
  (func (export "f64.informally_known_as_plain_snan") (result i64) (i64.reinterpret_f64 (f64.const nan:0x4000000000000)))
  (func (export "f64.all_ones_nan") (result i64) (i64.reinterpret_f64 (f64.const -nan:0xfffffffffffff)))
  (func (export "f64.misc_nan") (result i64) (i64.reinterpret_f64 (f64.const nan:0x0123456789abc)))
  (func (export "f64.misc_positive_nan") (result i64) (i64.reinterpret_f64 (f64.const +nan:0x3040506070809)))
  (func (export "f64.misc_negative_nan") (result i64) (i64.reinterpret_f64 (f64.const -nan:0x2abcdef012345)))
  (func (export "f64.infinity") (result i64) (i64.reinterpret_f64 (f64.const inf)))
  (func (export "f64.positive_infinity") (result i64) (i64.reinterpret_f64 (f64.const +inf)))
  (func (export "f64.negative_infinity") (result i64) (i64.reinterpret_f64 (f64.const -inf)))

  ;; f64 numbers
  (func (export "f64.zero") (result i64) (i64.reinterpret_f64 (f64.const 0x0.0p0)))
  (func (export "f64.positive_zero") (result i64) (i64.reinterpret_f64 (f64.const +0x0.0p0)))
  (func (export "f64.negative_zero") (result i64) (i64.reinterpret_f64 (f64.const -0x0.0p0)))
  (func (export "f64.misc") (result i64) (i64.reinterpret_f64 (f64.const 0x1.921fb54442d18p+2)))
  (func (export "f64.min_positive") (result i64) (i64.reinterpret_f64 (f64.const 0x0.0000000000001p-1022)))
  (func (export "f64.min_normal") (result i64) (i64.reinterpret_f64 (f64.const 0x1p-1022)))
  (func (export "f64.max_subnormal") (result i64) (i64.reinterpret_f64 (f64.const 0x0.fffffffffffffp-1022)))
  (func (export "f64.max_finite") (result i64) (i64.reinterpret_f64 (f64.const 0x1.fffffffffffffp+1023)))
  (func (export "f64.trailing_dot") (result i64) (i64.reinterpret_f64 (f64.const 0x1.p100)))

  ;; f64 numbers in decimal format
  (func (export "f64_dec.zero") (result i64) (i64.reinterpret_f64 (f64.const 0.0e0)))
  (func (export "f64_dec.positive_zero") (result i64) (i64.reinterpret_f64 (f64.const +0.0e0)))
  (func (export "f64_dec.negative_zero") (result i64) (i64.reinterpret_f64 (f64.const -0.0e0)))
  (func (export "f64_dec.misc") (result i64) (i64.reinterpret_f64 (f64.const 6.28318530717958623)))
  (func (export "f64_dec.min_positive") (result i64) (i64.reinterpret_f64 (f64.const 4.94066e-324)))
  (func (export "f64_dec.min_normal") (result i64) (i64.reinterpret_f64 (f64.const 2.2250738585072012e-308)))
  (func (export "f64_dec.max_subnormal") (result i64) (i64.reinterpret_f64 (f64.const 2.2250738585072011e-308)))
  (func (export "f64_dec.max_finite") (result i64) (i64.reinterpret_f64 (f64.const 1.7976931348623157e+308)))
  (func (export "f64_dec.trailing_dot") (result i64) (i64.reinterpret_f64 (f64.const 1.e100)))

  ;; https://twitter.com/Archivd/status/994637336506912768
  (func (export "f64_dec.root_beer_float") (result i64) (i64.reinterpret_f64 (f64.const 1.000000119)))

  (func (export "f32-dec-sep1") (result f32) (f32.const 1_000_000))
  (func (export "f32-dec-sep2") (result f32) (f32.const 1_0_0_0))
  (func (export "f32-dec-sep3") (result f32) (f32.const 100_3.141_592))
  (func (export "f32-dec-sep4") (result f32) (f32.const 99e+1_3))
  (func (export "f32-dec-sep5") (result f32) (f32.const 122_000.11_3_54E0_2_3))
  (func (export "f32-hex-sep1") (result f32) (f32.const 0xa_0f_00_99))
  (func (export "f32-hex-sep2") (result f32) (f32.const 0x1_a_A_0_f))
  (func (export "f32-hex-sep3") (result f32) (f32.const 0xa0_ff.f141_a59a))
  (func (export "f32-hex-sep4") (result f32) (f32.const 0xf0P+1_3))
  (func (export "f32-hex-sep5") (result f32) (f32.const 0x2a_f00a.1f_3_eep2_3))

  (func (export "f64-dec-sep1") (result f64) (f64.const 1_000_000))
  (func (export "f64-dec-sep2") (result f64) (f64.const 1_0_0_0))
  (func (export "f64-dec-sep3") (result f64) (f64.const 100_3.141_592))
  (func (export "f64-dec-sep4") (result f64) (f64.const 99e-1_23))
  (func (export "f64-dec-sep5") (result f64) (f64.const 122_000.11_3_54e0_2_3))
  (func (export "f64-hex-sep1") (result f64) (f64.const 0xa_f00f_0000_9999))
  (func (export "f64-hex-sep2") (result f64) (f64.const 0x1_a_A_0_f))
  (func (export "f64-hex-sep3") (result f64) (f64.const 0xa0_ff.f141_a59a))
  (func (export "f64-hex-sep4") (result f64) (f64.const 0xf0P+1_3))
  (func (export "f64-hex-sep5") (result f64) (f64.const 0x2a_f00a.1f_3_eep2_3))
)

(assert_return (invoke "f32.nan") (i32.const 0x7fc00000))
(assert_return (invoke "f32.positive_nan") (i32.const 0x7fc00000))
(assert_return (invoke "f32.negative_nan") (i32.const 0xffc00000))
(assert_return (invoke "f32.plain_nan") (i32.const 0x7fc00000))
(assert_return (invoke "f32.informally_known_as_plain_snan") (i32.const 0x7fa00000))
(assert_return (invoke "f32.all_ones_nan") (i32.const 0xffffffff))
(assert_return (invoke "f32.misc_nan") (i32.const 0x7f812345))
(assert_return (invoke "f32.misc_positive_nan") (i32.const 0x7fb04050))
(assert_return (invoke "f32.misc_negative_nan") (i32.const 0xffaabcde))
(assert_return (invoke "f32.infinity") (i32.const 0x7f800000))
(assert_return (invoke "f32.positive_infinity") (i32.const 0x7f800000))
(assert_return (invoke "f32.negative_infinity") (i32.const 0xff800000))
(assert_return (invoke "f32.zero") (i32.const 0))
(assert_return (invoke "f32.positive_zero") (i32.const 0))
(assert_return (invoke "f32.negative_zero") (i32.const 0x80000000))
(assert_return (invoke "f32.misc") (i32.const 0x40c90fdb))
(assert_return (invoke "f32.min_positive") (i32.const 1))
(assert_return (invoke "f32.min_normal") (i32.const 0x800000))
(assert_return (invoke "f32.max_subnormal") (i32.const 0x7fffff))
(assert_return (invoke "f32.max_finite") (i32.const 0x7f7fffff))
(assert_return (invoke "f32.trailing_dot") (i32.const 0x44800000))
(assert_return (invoke "f32_dec.zero") (i32.const 0))
(assert_return (invoke "f32_dec.positive_zero") (i32.const 0))
(assert_return (invoke "f32_dec.negative_zero") (i32.const 0x80000000))
(assert_return (invoke "f32_dec.misc") (i32.const 0x40c90fdb))
(assert_return (invoke "f32_dec.min_positive") (i32.const 1))
(assert_return (invoke "f32_dec.min_normal") (i32.const 0x800000))
(assert_return (invoke "f32_dec.max_subnormal") (i32.const 0x7fffff))
(assert_return (invoke "f32_dec.max_finite") (i32.const 0x7f7fffff))
(assert_return (invoke "f32_dec.trailing_dot") (i32.const 0x501502f9))
(assert_return (invoke "f32_dec.root_beer_float") (i32.const 0x3f800001))

(assert_return (invoke "f64.nan") (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.positive_nan") (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.negative_nan") (i64.const 0xfff8000000000000))
(assert_return (invoke "f64.plain_nan") (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.informally_known_as_plain_snan") (i64.const 0x7ff4000000000000))
(assert_return (invoke "f64.all_ones_nan") (i64.const 0xffffffffffffffff))
(assert_return (invoke "f64.misc_nan") (i64.const 0x7ff0123456789abc))
(assert_return (invoke "f64.misc_positive_nan") (i64.const 0x7ff3040506070809))
(assert_return (invoke "f64.misc_negative_nan") (i64.const 0xfff2abcdef012345))
(assert_return (invoke "f64.infinity") (i64.const 0x7ff0000000000000))
(assert_return (invoke "f64.positive_infinity") (i64.const 0x7ff0000000000000))
(assert_return (invoke "f64.negative_infinity") (i64.const 0xfff0000000000000))
(assert_return (invoke "f64.zero") (i64.const 0))
(assert_return (invoke "f64.positive_zero") (i64.const 0))
(assert_return (invoke "f64.negative_zero") (i64.const 0x8000000000000000))
(assert_return (invoke "f64.misc") (i64.const 0x401921fb54442d18))
(assert_return (invoke "f64.min_positive") (i64.const 1))
(assert_return (invoke "f64.min_normal") (i64.const 0x10000000000000))
(assert_return (invoke "f64.max_subnormal") (i64.const 0xfffffffffffff))
(assert_return (invoke "f64.max_finite") (i64.const 0x7fefffffffffffff))
(assert_return (invoke "f64.trailing_dot") (i64.const 0x4630000000000000))
(assert_return (invoke "f64_dec.zero") (i64.const 0))
(assert_return (invoke "f64_dec.positive_zero") (i64.const 0))
(assert_return (invoke "f64_dec.negative_zero") (i64.const 0x8000000000000000))
(assert_return (invoke "f64_dec.misc") (i64.const 0x401921fb54442d18))
(assert_return (invoke "f64_dec.min_positive") (i64.const 1))
(assert_return (invoke "f64_dec.min_normal") (i64.const 0x10000000000000))
(assert_return (invoke "f64_dec.max_subnormal") (i64.const 0xfffffffffffff))
(assert_return (invoke "f64_dec.max_finite") (i64.const 0x7fefffffffffffff))
(assert_return (invoke "f64_dec.trailing_dot") (i64.const 0x54b249ad2594c37d))
(assert_return (invoke "f64_dec.root_beer_float") (i64.const 0x3ff000001ff19e24))

(assert_return (invoke "f32-dec-sep1") (f32.const 1000000))
(assert_return (invoke "f32-dec-sep2") (f32.const 1000))
(assert_return (invoke "f32-dec-sep3") (f32.const 1003.141592))
(assert_return (invoke "f32-dec-sep4") (f32.const 99e+13))
(assert_return (invoke "f32-dec-sep5") (f32.const 122000.11354e23))
(assert_return (invoke "f32-hex-sep1") (f32.const 0xa0f0099))
(assert_return (invoke "f32-hex-sep2") (f32.const 0x1aa0f))
(assert_return (invoke "f32-hex-sep3") (f32.const 0xa0ff.f141a59a))
(assert_return (invoke "f32-hex-sep4") (f32.const 0xf0P+13))
(assert_return (invoke "f32-hex-sep5") (f32.const 0x2af00a.1f3eep23))

(assert_return (invoke "f64-dec-sep1") (f64.const 1000000))
(assert_return (invoke "f64-dec-sep2") (f64.const 1000))
(assert_return (invoke "f64-dec-sep3") (f64.const 1003.141592))
(assert_return (invoke "f64-dec-sep4") (f64.const 99e-123))
(assert_return (invoke "f64-dec-sep5") (f64.const 122000.11354e23))
(assert_return (invoke "f64-hex-sep1") (f64.const 0xaf00f00009999))
(assert_return (invoke "f64-hex-sep2") (f64.const 0x1aa0f))
(assert_return (invoke "f64-hex-sep3") (f64.const 0xa0ff.f141a59a))
(assert_return (invoke "f64-hex-sep4") (f64.const 0xf0P+13))
(assert_return (invoke "f64-hex-sep5") (f64.const 0x2af00a.1f3eep23))

;; Test parsing a float from binary
(module binary
  ;; (func (export "4294967249") (result f64) (f64.const 4294967249))
  "\00\61\73\6d\01\00\00\00\01\85\80\80\80\00\01\60"
  "\00\01\7c\03\82\80\80\80\00\01\00\07\8e\80\80\80"
  "\00\01\0a\34\32\39\34\39\36\37\32\34\39\00\00\0a"
  "\91\80\80\80\00\01\8b\80\80\80\00\00\44\00\00\20"
  "\fa\ff\ff\ef\41\0b"
)

(assert_return (invoke "4294967249") (f64.const 4294967249))

(assert_malformed
  (module quote "(global f32 (f32.const _100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const +_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const -_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 99_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1__000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const _1.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1.0_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1_.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1._0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const _1e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1e1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1_e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1e_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const _1.0e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1.0e1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1.0_e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1.0e_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1.0e+_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 1.0e_+1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const _0x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0_x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x00_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0xff__ffff))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x_1.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1.0_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1_.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1._0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x_1p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1p1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1_p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1p_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x_1.0p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1.0p1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1.0_p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1.0p_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1.0p+_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f32 (f32.const 0x1.0p_+1))")
  "unknown operator"
)

(assert_malformed
  (module quote "(global f64 (f64.const _100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const +_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const -_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 99_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1__000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const _1.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1.0_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1_.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1._0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const _1e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1e1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1_e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1e_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const _1.0e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1.0e1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1.0_e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1.0e_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1.0e+_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 1.0e_+1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const _0x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0_x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x00_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0xff__ffff))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x_1.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1.0_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1_.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1._0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x_1p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1p1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1_p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1p_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x_1.0p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1.0p1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1.0_p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1.0p_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1.0p+_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global f64 (f64.const 0x1.0p_+1))")
  "unknown operator"
)
