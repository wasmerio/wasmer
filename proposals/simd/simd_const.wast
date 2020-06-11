;; v128.const normal parameter (e.g. (i8x16, i16x8 i32x4, f32x4))

(module (func (v128.const i8x16  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF) drop))
(module (func (v128.const i8x16 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80) drop))
(module (func (v128.const i8x16  255  255  255  255  255  255  255  255  255  255  255  255  255  255  255  255) drop))
(module (func (v128.const i8x16 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128) drop))
(module (func (v128.const i16x8  0xFFFF  0xFFFF  0xFFFF  0xFFFF  0xFFFF  0xFFFF  0xFFFF  0xFFFF) drop))
(module (func (v128.const i16x8 -0x8000 -0x8000 -0x8000 -0x8000 -0x8000 -0x8000 -0x8000 -0x8000) drop))
(module (func (v128.const i16x8  65535  65535  65535  65535  65535  65535  65535  65535) drop))
(module (func (v128.const i16x8 -32768 -32768 -32768 -32768 -32768 -32768 -32768 -32768) drop))
(module (func (v128.const i16x8  65_535  65_535  65_535  65_535  65_535  65_535  65_535  65_535) drop))
(module (func (v128.const i16x8 -32_768 -32_768 -32_768 -32_768 -32_768 -32_768 -32_768 -32_768) drop))
(module (func (v128.const i16x8  0_123_45 0_123_45 0_123_45 0_123_45 0_123_45 0_123_45 0_123_45 0_123_45) drop))
(module (func (v128.const i16x8  0x0_1234 0x0_1234 0x0_1234 0x0_1234 0x0_1234 0x0_1234 0x0_1234 0x0_1234) drop))
(module (func (v128.const i32x4  0xffffffff  0xffffffff  0xffffffff  0xffffffff) drop))
(module (func (v128.const i32x4 -0x80000000 -0x80000000 -0x80000000 -0x80000000) drop))
(module (func (v128.const i32x4  4294967295  4294967295  4294967295  4294967295) drop))
(module (func (v128.const i32x4 -2147483648 -2147483648 -2147483648 -2147483648) drop))
(module (func (v128.const i32x4  0xffff_ffff  0xffff_ffff  0xffff_ffff  0xffff_ffff) drop))
(module (func (v128.const i32x4 -0x8000_0000 -0x8000_0000 -0x8000_0000 -0x8000_0000) drop))
(module (func (v128.const i32x4 4_294_967_295  4_294_967_295  4_294_967_295  4_294_967_295) drop))
(module (func (v128.const i32x4 -2_147_483_648 -2_147_483_648 -2_147_483_648 -2_147_483_648) drop))
(module (func (v128.const i32x4 0_123_456_789 0_123_456_789 0_123_456_789 0_123_456_789) drop))
(module (func (v128.const i32x4 0x0_9acf_fBDF 0x0_9acf_fBDF 0x0_9acf_fBDF 0x0_9acf_fBDF) drop))
(module (func (v128.const i64x2  0xffffffffffffffff  0xffffffffffffffff) drop))
(module (func (v128.const i64x2 -0x8000000000000000 -0x8000000000000000) drop))
(module (func (v128.const i64x2  18446744073709551615 18446744073709551615) drop))
(module (func (v128.const i64x2 -9223372036854775808 -9223372036854775808) drop))
(module (func (v128.const i64x2  0xffff_ffff_ffff_ffff  0xffff_ffff_ffff_ffff) drop))
(module (func (v128.const i64x2 -0x8000_0000_0000_0000 -0x8000_0000_0000_0000) drop))
(module (func (v128.const i64x2  18_446_744_073_709_551_615 18_446_744_073_709_551_615) drop))
(module (func (v128.const i64x2 -9_223_372_036_854_775_808 -9_223_372_036_854_775_808) drop))
(module (func (v128.const i64x2  0_123_456_789 0_123_456_789) drop))
(module (func (v128.const i64x2  0x0125_6789_ADEF_bcef 0x0125_6789_ADEF_bcef) drop))
(module (func (v128.const f32x4  0x1p127  0x1p127  0x1p127  0x1p127) drop))
(module (func (v128.const f32x4 -0x1p127 -0x1p127 -0x1p127 -0x1p127) drop))
(module (func (v128.const f32x4  1e38  1e38  1e38  1e38) drop))
(module (func (v128.const f32x4 -1e38 -1e38 -1e38 -1e38) drop))
(module (func (v128.const f32x4  340282356779733623858607532500980858880 340282356779733623858607532500980858880
                                 340282356779733623858607532500980858880 340282356779733623858607532500980858880) drop))
(module (func (v128.const f32x4 -340282356779733623858607532500980858880 -340282356779733623858607532500980858880
                                -340282356779733623858607532500980858880 -340282356779733623858607532500980858880) drop))
(module (func (v128.const f32x4 nan:0x1 nan:0x1 nan:0x1 nan:0x1) drop))
(module (func (v128.const f32x4 nan:0x7f_ffff nan:0x7f_ffff nan:0x7f_ffff nan:0x7f_ffff) drop))
(module (func (v128.const f32x4 0123456789 0123456789 0123456789 0123456789) drop))
(module (func (v128.const f32x4 0123456789e019 0123456789e019 0123456789e019 0123456789e019) drop))
(module (func (v128.const f32x4 0123456789e+019 0123456789e+019 0123456789e+019 0123456789e+019) drop))
(module (func (v128.const f32x4 0123456789e-019 0123456789e-019 0123456789e-019 0123456789e-019) drop))
(module (func (v128.const f32x4 0123456789. 0123456789. 0123456789. 0123456789.) drop))
(module (func (v128.const f32x4 0123456789.e019 0123456789.e019 0123456789.e019 0123456789.e019) drop))
(module (func (v128.const f32x4 0123456789.e+019 0123456789.e+019 0123456789.e+019 0123456789.e+019) drop))
(module (func (v128.const f32x4 0123456789.e-019 0123456789.e-019 0123456789.e-019 0123456789.e-019) drop))
(module (func (v128.const f32x4 0123456789.0123456789 0123456789.0123456789 0123456789.0123456789 0123456789.0123456789) drop))
(module (func (v128.const f32x4 0123456789.0123456789e019 0123456789.0123456789e019 0123456789.0123456789e019 0123456789.0123456789e019) drop))
(module (func (v128.const f32x4 0123456789.0123456789e+019 0123456789.0123456789e+019 0123456789.0123456789e+019 0123456789.0123456789e+019) drop))
(module (func (v128.const f32x4 0123456789.0123456789e-019 0123456789.0123456789e-019 0123456789.0123456789e-019 0123456789.0123456789e-019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF 0x0123456789ABCDEF 0x0123456789ABCDEF 0x0123456789ABCDEF) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEFp019 0x0123456789ABCDEFp019 0x0123456789ABCDEFp019 0x0123456789ABCDEFp019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEFp+019 0x0123456789ABCDEFp+019 0x0123456789ABCDEFp+019 0x0123456789ABCDEFp+019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEFp-019 0x0123456789ABCDEFp-019 0x0123456789ABCDEFp-019 0x0123456789ABCDEFp-019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF. 0x0123456789ABCDEF. 0x0123456789ABCDEF. 0x0123456789ABCDEF.) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF.p019 0x0123456789ABCDEF.p019 0x0123456789ABCDEF.p019 0x0123456789ABCDEF.p019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF.p+019 0x0123456789ABCDEF.p+019 0x0123456789ABCDEF.p+019 0x0123456789ABCDEF.p+019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF.p-019 0x0123456789ABCDEF.p-019 0x0123456789ABCDEF.p-019 0x0123456789ABCDEF.p-019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF.019aF 0x0123456789ABCDEF.019aF 0x0123456789ABCDEF.019aF 0x0123456789ABCDEF.019aF) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF.019aFp019 0x0123456789ABCDEF.019aFp019 0x0123456789ABCDEF.019aFp019 0x0123456789ABCDEF.019aFp019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF.019aFp+019 0x0123456789ABCDEF.019aFp+019 0x0123456789ABCDEF.019aFp+019 0x0123456789ABCDEF.019aFp+019) drop))
(module (func (v128.const f32x4 0x0123456789ABCDEF.019aFp-019 0x0123456789ABCDEF.019aFp-019 0x0123456789ABCDEF.019aFp-019 0x0123456789ABCDEF.019aFp-019) drop))
(module (func (v128.const f64x2  0x1p1023  0x1p1023) drop))
(module (func (v128.const f64x2 -0x1p1023 -0x1p1023) drop))
(module (func (v128.const f64x2  1e308  1e308) drop))
(module (func (v128.const f64x2 -1e308 -1e308) drop))
(module (func (v128.const f64x2  179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368
                                 179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368) drop))
(module (func (v128.const f64x2 -179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368
                                -179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368) drop))
(module (func (v128.const f64x2 nan:0x1 nan:0x1) drop))
(module (func (v128.const f64x2 nan:0xf_ffff_ffff_ffff nan:0xf_ffff_ffff_ffff) drop))
(module (func (v128.const f64x2 0123456789 0123456789) drop))
(module (func (v128.const f64x2 0123456789e019 0123456789e019) drop))
(module (func (v128.const f64x2 0123456789e+019 0123456789e+019) drop))
(module (func (v128.const f64x2 0123456789e-019 0123456789e-019) drop))
(module (func (v128.const f64x2 0123456789. 0123456789.) drop))
(module (func (v128.const f64x2 0123456789.e019 0123456789.e019) drop))
(module (func (v128.const f64x2 0123456789.e+019 0123456789.e+019) drop))
(module (func (v128.const f64x2 0123456789.e-019 0123456789.e-019) drop))
(module (func (v128.const f64x2 0123456789.0123456789 0123456789.0123456789) drop))
(module (func (v128.const f64x2 0123456789.0123456789e019 0123456789.0123456789e019) drop))
(module (func (v128.const f64x2 0123456789.0123456789e+019 0123456789.0123456789e+019) drop))
(module (func (v128.const f64x2 0123456789.0123456789e-019 0123456789.0123456789e-019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef 0x0123456789ABCDEFabcdef) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdefp019 0x0123456789ABCDEFabcdefp019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdefp+019 0x0123456789ABCDEFabcdefp+019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdefp-019 0x0123456789ABCDEFabcdefp-019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef. 0x0123456789ABCDEFabcdef.) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef.p019 0x0123456789ABCDEFabcdef.p019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef.p+019 0x0123456789ABCDEFabcdef.p+019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef.p-019 0x0123456789ABCDEFabcdef.p-019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdef 0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdef) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp019 0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp+019 0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp+019) drop))
(module (func (v128.const f64x2 0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp-019 0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp-019) drop))

;; Non-splat cases

(module (func (v128.const i8x16  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF  0xFF
                                -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80) drop))
(module (func (v128.const i8x16  0xFF  0xFF  0xFF  0xFF   255   255   255   255
                                -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80 -0x80) drop))
(module (func (v128.const i8x16  0xFF  0xFF  0xFF  0xFF   255   255   255   255
                                -0x80 -0x80 -0x80 -0x80  -128  -128  -128  -128) drop))
(module (func (v128.const i16x8 0xFF 0xFF  0xFF  0xFF -0x8000 -0x8000 -0x8000 -0x8000) drop))
(module (func (v128.const i16x8 0xFF 0xFF 65535 65535 -0x8000 -0x8000 -0x8000 -0x8000) drop))
(module (func (v128.const i16x8 0xFF 0xFF 65535 65535 -0x8000 -0x8000  -32768  -32768) drop))
(module (func (v128.const i32x4 0xffffffff 0xffffffff -0x80000000 -0x80000000) drop))
(module (func (v128.const i32x4 0xffffffff 4294967295 -0x80000000 -0x80000000) drop))
(module (func (v128.const i32x4 0xffffffff 4294967295 -0x80000000 -2147483648) drop))
(module (func (v128.const f32x4 0x1p127 0x1p127 -0x1p127 -1e38) drop))
(module (func (v128.const f32x4 0x1p127 340282356779733623858607532500980858880 -1e38 -340282356779733623858607532500980858880) drop))
(module (func (v128.const f32x4 nan -nan inf -inf) drop))
(module (func (v128.const i64x2 0xffffffffffffffff 0x8000000000000000) drop))
(module (func (v128.const i64x2 0xffffffffffffffff -9223372036854775808) drop))
(module (func (v128.const f64x2 0x1p1023 -1e308) drop))
(module (func (v128.const f64x2 nan -inf) drop))

;; Constant out of range (int literal is too large)

(module (memory 1))
(assert_malformed
  (module quote "(func (v128.const i8x16 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100 0x100) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i8x16 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81 -0x81) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i8x16 256 256 256 256 256 256 256 256 256 256 256 256 256 256 256 256) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i8x16 -129 -129 -129 -129 -129 -129 -129 -129 -129 -129 -129 -129 -129 -129 -129 -129) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i16x8 0x10000 0x10000 0x10000 0x10000 0x10000 0x10000 0x10000 0x10000) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i16x8 -0x8001 -0x8001 -0x8001 -0x8001 -0x8001 -0x8001 -0x8001 -0x8001) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i16x8 65536 65536 65536 65536 65536 65536 65536 65536) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i16x8 -32769 -32769 -32769 -32769 -32769 -32769 -32769 -32769) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i32x4  0x100000000  0x100000000  0x100000000  0x100000000) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i32x4 -0x80000001 -0x80000001 -0x80000001 -0x80000001) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i32x4  4294967296  4294967296  4294967296  4294967296) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const i32x4 -2147483649 -2147483649 -2147483649 -2147483649) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const f32x4  0x1p128  0x1p128  0x1p128  0x1p128) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 -0x1p128 -0x1p128 -0x1p128 -0x1p128) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const f32x4  1e39  1e39  1e39  1e39) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 -1e39 -1e39 -1e39 -1e39) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const f32x4  340282356779733661637539395458142568448 340282356779733661637539395458142568448"
                "                         340282356779733661637539395458142568448 340282356779733661637539395458142568448) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 -340282356779733661637539395458142568448 -340282356779733661637539395458142568448"
                "                        -340282356779733661637539395458142568448 -340282356779733661637539395458142568448) drop)")
  "constant out of range"
)

(assert_malformed
  (module quote "(func (v128.const f32x4 nan:0x80_0000 nan:0x80_0000 nan:0x80_0000 nan:0x80_0000) drop)")
  "constant out of range"
)

(assert_malformed
  (module quote "(func (v128.const f64x2  269653970229347356221791135597556535197105851288767494898376215204735891170042808140884337949150317257310688430271573696351481990334196274152701320055306275479074865864826923114368235135583993416113802762682700913456874855354834422248712838998185022412196739306217084753107265771378949821875606039276187287552"
                "                         269653970229347356221791135597556535197105851288767494898376215204735891170042808140884337949150317257310688430271573696351481990334196274152701320055306275479074865864826923114368235135583993416113802762682700913456874855354834422248712838998185022412196739306217084753107265771378949821875606039276187287552) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 -269653970229347356221791135597556535197105851288767494898376215204735891170042808140884337949150317257310688430271573696351481990334196274152701320055306275479074865864826923114368235135583993416113802762682700913456874855354834422248712838998185022412196739306217084753107265771378949821875606039276187287552"
                "                        -269653970229347356221791135597556535197105851288767494898376215204735891170042808140884337949150317257310688430271573696351481990334196274152701320055306275479074865864826923114368235135583993416113802762682700913456874855354834422248712838998185022412196739306217084753107265771378949821875606039276187287552) drop)")
  "constant out of range"
)

(assert_malformed
  (module quote "(func (v128.const f64x2 nan:0x10_0000_0000_0000 nan:0x10_0000_0000_0000) drop)")
  "constant out of range"
)

;; More malformed v128.const forms
(assert_malformed
  (module quote "(func (v128.const) drop)")
  "unexpected token"
)

(assert_malformed
  (module quote "(func (v128.const 0 0 0 0) drop)")
  "unexpected token"
)
(assert_malformed
  (module quote "(func (v128.const i8x16) drop)")
  "wrong number of lane literals"
)
(assert_malformed
  (module quote "(func (v128.const i8x16 0x 0x 0x 0x 0x 0x 0x 0x 0x 0x 0x 0x 0x 0x 0x 0x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const i8x16 1x 1x 1x 1x 1x 1x 1x 1x 1x 1x 1x 1x 1x 1x 1x 1x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const i8x16 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg) drop)")
  "unknown operator"
)

(assert_malformed
  (module quote "(func (v128.const i16x8) drop)")
  "wrong number of lane literals"
)
(assert_malformed
  (module quote "(func (v128.const i16x8 0x 0x 0x 0x 0x 0x 0x 0x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const i16x8 1x 1x 1x 1x 1x 1x 1x 1x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const i16x8 0xg 0xg 0xg 0xg 0xg 0xg 0xg 0xg) drop)")
  "unknown operator"
)

(assert_malformed
  (module quote "(func (v128.const i32x4) drop)")
  "wrong number of lane literals"
)
(assert_malformed
  (module quote "(func (v128.const i32x4 0x 0x 0x 0x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const i32x4 1x 1x 1x 1x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const i32x4 0xg 0xg 0xg 0xg) drop)")
  "unknown operator"
)

(assert_malformed
  (module quote "(func (v128.const i64x2) drop)")
  "wrong number of lane literals"
)
(assert_malformed
  (module quote "(func (v128.const i64x2 0x 0x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 1x 1x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0xg 0xg) drop)")
  "unknown operator"
)

(assert_malformed
  (module quote "(func (v128.const f32x4) drop)")
  "wrong number of lane literals"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 .0 .0 .0 .0) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 .0e0 .0e0 .0e0 .0e0) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0e 0e 0e 0e) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0e+ 0e+ 0e+ 0e+) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0.0e 0.0e 0.0e 0.0e) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0.0e- 0.0e- 0.0e- 0.0e-) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x 0x 0x 0x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 1x 1x 1x 1x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0xg 0xg 0xg 0xg) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x. 0x. 0x. 0x.) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x0.g 0x0.g 0x0.g 0x0.g) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x0p 0x0p 0x0p 0x0p) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x0p+ 0x0p+ 0x0p+ 0x0p+) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x0p- 0x0p- 0x0p- 0x0p-) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x0.0p 0x0.0p 0x0.0p 0x0.0p) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x0.0p+ 0x0.0p+ 0x0.0p+ 0x0.0p+) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x0.0p- 0x0.0p- 0x0.0p- 0x0.0p-) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 0x0pA 0x0pA 0x0pA 0x0pA) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 nan:1 nan:1 nan:1 nan:1) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f32x4 nan:0x0 nan:0x0 nan:0x0 nan:0x0) drop)")
  "constant out of range"
)

(assert_malformed
  (module quote "(func (v128.const f64x2) drop)")
  "wrong number of lane literals"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 .0 .0) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 .0e0 .0e0) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0e 0e) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0e+ 0e+) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0.0e+ 0.0e+) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0.0e- 0.0e-) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x 0x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 1x 1x) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0xg 0xg) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x. 0x.) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x0.g 0x0.g) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x0p 0x0p) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x0p+ 0x0p+) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x0p- 0x0p-) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x0.0p 0x0.0p) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x0.0p+ 0x0.0p+) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x0.0p- 0x0.0p-) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 0x0pA 0x0pA) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 nan:1 nan:1) drop)")
  "unknown operator"
)
(assert_malformed
  (module quote "(func (v128.const f64x2 nan:0x0 nan:0x0) drop)")
  "constant out of range"
)

;; too little arguments

(assert_malformed
  (module quote "(func (v128.const i32x4 0x10000000000000000 0x10000000000000000) drop)")
  "wrong number of lane literals"
)

;; too many arguments
(assert_malformed
  (module quote "(func (v128.const i32x4 0x1 0x1 0x1 0x1 0x1) drop)")
  "wrong number of lane literals"
)

;; Rounding behaviour

;; f32x4, small exponent
(module (func (export "f") (result v128) (v128.const f32x4 +0x1.00000100000000000p-50 +0x1.00000100000000000p-50 +0x1.00000100000000000p-50 +0x1.00000100000000000p-50)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000000p-50 +0x1.000000p-50 +0x1.000000p-50 +0x1.000000p-50))
(module (func (export "f") (result v128) (v128.const f32x4 -0x1.00000100000000000p-50 -0x1.00000100000000000p-50 -0x1.00000100000000000p-50 -0x1.00000100000000000p-50)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000000p-50 -0x1.000000p-50 -0x1.000000p-50 -0x1.000000p-50))
(module (func (export "f") (result v128) (v128.const f32x4 +0x1.00000500000000001p-50 +0x1.00000500000000001p-50 +0x1.00000500000000001p-50 +0x1.00000500000000001p-50)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000006p-50 +0x1.000006p-50 +0x1.000006p-50 +0x1.000006p-50))
(module (func (export "f") (result v128) (v128.const f32x4 -0x1.00000500000000001p-50 -0x1.00000500000000001p-50 -0x1.00000500000000001p-50 -0x1.00000500000000001p-50)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000006p-50 -0x1.000006p-50 -0x1.000006p-50 -0x1.000006p-50))

(module (func (export "f") (result v128) (v128.const f32x4 +0x4000.004000000p-64 +0x4000.004000000p-64 +0x4000.004000000p-64 +0x4000.004000000p-64)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000000p-50 +0x1.000000p-50 +0x1.000000p-50 +0x1.000000p-50))
(module (func (export "f") (result v128) (v128.const f32x4 -0x4000.004000000p-64 -0x4000.004000000p-64 -0x4000.004000000p-64 -0x4000.004000000p-64)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000000p-50 -0x1.000000p-50 -0x1.000000p-50 -0x1.000000p-50))
(module (func (export "f") (result v128) (v128.const f32x4 +0x4000.014000001p-64 +0x4000.014000001p-64 +0x4000.014000001p-64 +0x4000.014000001p-64)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000006p-50 +0x1.000006p-50 +0x1.000006p-50 +0x1.000006p-50))
(module (func (export "f") (result v128) (v128.const f32x4 -0x4000.014000001p-64 -0x4000.014000001p-64 -0x4000.014000001p-64 -0x4000.014000001p-64)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000006p-50 -0x1.000006p-50 -0x1.000006p-50 -0x1.000006p-50))

(module (func (export "f") (result v128) (v128.const f32x4 +8.8817847263968443573e-16 +8.8817847263968443573e-16 +8.8817847263968443573e-16 +8.8817847263968443573e-16)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000000p-50 +0x1.000000p-50 +0x1.000000p-50 +0x1.000000p-50))
(module (func (export "f") (result v128) (v128.const f32x4 -8.8817847263968443573e-16 -8.8817847263968443573e-16 -8.8817847263968443573e-16 -8.8817847263968443573e-16)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000000p-50 -0x1.000000p-50 -0x1.000000p-50 -0x1.000000p-50))
(module (func (export "f") (result v128) (v128.const f32x4 +8.8817857851880284253e-16 +8.8817857851880284253e-16 +8.8817857851880284253e-16 +8.8817857851880284253e-16)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000004p-50 +0x1.000004p-50 +0x1.000004p-50 +0x1.000004p-50))
(module (func (export "f") (result v128) (v128.const f32x4 -8.8817857851880284253e-16 -8.8817857851880284253e-16 -8.8817857851880284253e-16 -8.8817857851880284253e-16)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000004p-50 -0x1.000004p-50 -0x1.000004p-50 -0x1.000004p-50))

;; f32x4, large exponent
(module (func (export "f") (result v128) (v128.const f32x4 +0x1.00000100000000000p+50 +0x1.00000100000000000p+50 +0x1.00000100000000000p+50 +0x1.00000100000000000p+50)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000000p+50 +0x1.000000p+50 +0x1.000000p+50 +0x1.000000p+50))
(module (func (export "f") (result v128) (v128.const f32x4 -0x1.00000100000000000p+50 -0x1.00000100000000000p+50 -0x1.00000100000000000p+50 -0x1.00000100000000000p+50)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000000p+50 -0x1.000000p+50 -0x1.000000p+50 -0x1.000000p+50))
(module (func (export "f") (result v128) (v128.const f32x4 +0x1.00000500000000001p+50 +0x1.00000500000000001p+50 +0x1.00000500000000001p+50 +0x1.00000500000000001p+50)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000006p+50 +0x1.000006p+50 +0x1.000006p+50 +0x1.000006p+50))
(module (func (export "f") (result v128) (v128.const f32x4 -0x1.00000500000000001p+50 -0x1.00000500000000001p+50 -0x1.00000500000000001p+50 -0x1.00000500000000001p+50)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000006p+50 -0x1.000006p+50 -0x1.000006p+50 -0x1.000006p+50))

(module (func (export "f") (result v128) (v128.const f32x4 +0x4000004000000 +0x4000004000000 +0x4000004000000 +0x4000004000000)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000000p+50 +0x1.000000p+50 +0x1.000000p+50 +0x1.000000p+50))
(module (func (export "f") (result v128) (v128.const f32x4 -0x4000004000000 -0x4000004000000 -0x4000004000000 -0x4000004000000)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000000p+50 -0x1.000000p+50 -0x1.000000p+50 -0x1.000000p+50))
(module (func (export "f") (result v128) (v128.const f32x4 +0x400000c000000 +0x400000c000000 +0x400000c000000 +0x400000c000000)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000004p+50 +0x1.000004p+50 +0x1.000004p+50 +0x1.000004p+50))
(module (func (export "f") (result v128) (v128.const f32x4 -0x400000c000000 -0x400000c000000 -0x400000c000000 -0x400000c000000)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000004p+50 -0x1.000004p+50 -0x1.000004p+50 -0x1.000004p+50))

(module (func (export "f") (result v128) (v128.const f32x4 +1125899973951488 +1125899973951488 +1125899973951488 +1125899973951488)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000000p+50 +0x1.000000p+50 +0x1.000000p+50 +0x1.000000p+50))
(module (func (export "f") (result v128) (v128.const f32x4 -1125899973951488 -1125899973951488 -1125899973951488 -1125899973951488)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000000p+50 -0x1.000000p+50 -0x1.000000p+50 -0x1.000000p+50))
(module (func (export "f") (result v128) (v128.const f32x4 +1125900108169216 +1125900108169216 +1125900108169216 +1125900108169216)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.000004p+50 +0x1.000004p+50 +0x1.000004p+50 +0x1.000004p+50))
(module (func (export "f") (result v128) (v128.const f32x4 -1125900108169216 -1125900108169216 -1125900108169216 -1125900108169216)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.000004p+50 -0x1.000004p+50 -0x1.000004p+50 -0x1.000004p+50))

;; f32x4, subnormal
(module (func (export "f") (result v128) (v128.const f32x4 +0x0.00000100000000000p-126 +0x0.00000100000000000p-126 +0x0.00000100000000000p-126 +0x0.00000100000000000p-126)))
(assert_return (invoke "f") (v128.const f32x4 +0x0.000000p-126 +0x0.000000p-126 +0x0.000000p-126 +0x0.000000p-126))
(module (func (export "f") (result v128) (v128.const f32x4 -0x0.00000100000000000p-126 -0x0.00000100000000000p-126 -0x0.00000100000000000p-126 -0x0.00000100000000000p-126)))
(assert_return (invoke "f") (v128.const f32x4 -0x0.000000p-126 -0x0.000000p-126 -0x0.000000p-126 -0x0.000000p-126))
(module (func (export "f") (result v128) (v128.const f32x4 +0x0.00000500000000001p-126 +0x0.00000500000000001p-126 +0x0.00000500000000001p-126 +0x0.00000500000000001p-126)))
(assert_return (invoke "f") (v128.const f32x4 +0x0.000006p-126 +0x0.000006p-126 +0x0.000006p-126 +0x0.000006p-126))
(module (func (export "f") (result v128) (v128.const f32x4 -0x0.00000500000000001p-126 -0x0.00000500000000001p-126 -0x0.00000500000000001p-126 -0x0.00000500000000001p-126)))
(assert_return (invoke "f") (v128.const f32x4 -0x0.000006p-126 -0x0.000006p-126 -0x0.000006p-126 -0x0.000006p-126))

;; f32x4, round down at limit to infinity
(module (func (export "f") (result v128) (v128.const f32x4 +0x1.fffffe8p127 +0x1.fffffe8p127 +0x1.fffffe8p127 +0x1.fffffe8p127)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.fffffep127 +0x1.fffffep127 +0x1.fffffep127 +0x1.fffffep127))
(module (func (export "f") (result v128) (v128.const f32x4 -0x1.fffffe8p127 -0x1.fffffe8p127 -0x1.fffffe8p127 -0x1.fffffe8p127)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.fffffep127 -0x1.fffffep127 -0x1.fffffep127 -0x1.fffffep127))
(module (func (export "f") (result v128) (v128.const f32x4 +0x1.fffffefffffffffffp127 +0x1.fffffefffffffffffp127 +0x1.fffffefffffffffffp127 +0x1.fffffefffffffffffp127)))
(assert_return (invoke "f") (v128.const f32x4 +0x1.fffffep127 +0x1.fffffep127 +0x1.fffffep127 +0x1.fffffep127))
(module (func (export "f") (result v128) (v128.const f32x4 -0x1.fffffefffffffffffp127 -0x1.fffffefffffffffffp127 -0x1.fffffefffffffffffp127 -0x1.fffffefffffffffffp127)))
(assert_return (invoke "f") (v128.const f32x4 -0x1.fffffep127 -0x1.fffffep127 -0x1.fffffep127 -0x1.fffffep127))

;; f64x2, small exponent
(module (func (export "f") (result f64) (f64.const +0x1.000000000000080000000000p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000000p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000080000000000p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000000p-600))
(module (func (export "f") (result f64) (f64.const +0x1.000000000000080000000001p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000080000000001p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x1.0000000000000fffffffffffp-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x1.0000000000000fffffffffffp-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x1.000000000000100000000000p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000100000000000p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x1.000000000000100000000001p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000100000000001p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x1.00000000000017ffffffffffp-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x1.00000000000017ffffffffffp-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x1.000000000000180000000000p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000180000000000p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x1.000000000000180000000001p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000180000000001p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x1.0000000000001fffffffffffp-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x1.0000000000001fffffffffffp-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x1.000000000000200000000000p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000200000000000p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x1.000000000000200000000001p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000200000000001p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x1.00000000000027ffffffffffp-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x1.00000000000027ffffffffffp-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x1.000000000000280000000001p-600)))
(assert_return (invoke "f") (f64.const +0x1.0000000000003p-600))
(module (func (export "f") (result f64) (f64.const -0x1.000000000000280000000001p-600)))
(assert_return (invoke "f") (f64.const -0x1.0000000000003p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000000400000000000p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000000p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000000400000000000p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000000p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000000400000000001p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000000400000000001p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.0000007fffffffffffp-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.0000007fffffffffffp-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000000800000000000p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000000800000000000p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000000800000000001p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000000800000000001p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000000bfffffffffffp-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000000bfffffffffffp-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000000c00000000000p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000000c00000000000p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000000c00000000001p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000000c00000000001p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000000ffffffffffffp-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000000ffffffffffffp-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000001000000000000p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000001000000000000p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000001000000000001p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000001000000000001p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.0000013fffffffffffp-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.0000013fffffffffffp-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p-600))
(module (func (export "f") (result f64) (f64.const +0x8000000.000001400000000001p-627)))
(assert_return (invoke "f") (f64.const +0x1.0000000000003p-600))
(module (func (export "f") (result f64) (f64.const -0x8000000.000001400000000001p-627)))
(assert_return (invoke "f") (f64.const -0x1.0000000000003p-600))
(module (func (export "f") (result f64) (f64.const +5.3575430359313371995e+300)))
(assert_return (invoke "f") (f64.const +0x1.0000000000000p+999))
(module (func (export "f") (result f64) (f64.const -5.3575430359313371995e+300)))
(assert_return (invoke "f") (f64.const -0x1.0000000000000p+999))
(module (func (export "f") (result f64) (f64.const +5.3575430359313371996e+300)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p+999))
(module (func (export "f") (result f64) (f64.const -5.3575430359313371996e+300)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p+999))
(module (func (export "f") (result f64) (f64.const +5.3575430359313383891e+300)))
(assert_return (invoke "f") (f64.const +0x1.0000000000001p+999))
(module (func (export "f") (result f64) (f64.const -5.3575430359313383891e+300)))
(assert_return (invoke "f") (f64.const -0x1.0000000000001p+999))
(module (func (export "f") (result f64) (f64.const +5.3575430359313383892e+300)))
(assert_return (invoke "f") (f64.const +0x1.0000000000002p+999))
(module (func (export "f") (result f64) (f64.const -5.3575430359313383892e+300)))
(assert_return (invoke "f") (f64.const -0x1.0000000000002p+999))

;; f64, large exponent
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000080000000000p+600 +0x1.000000000000080000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000000p+600 +0x1.0000000000000p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000080000000000p+600 -0x1.000000000000080000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000000p+600 -0x1.0000000000000p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000080000000001p+600 +0x1.000000000000080000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+600 +0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000080000000001p+600 -0x1.000000000000080000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+600 -0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.0000000000000fffffffffffp+600 +0x1.0000000000000fffffffffffp+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+600 +0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.0000000000000fffffffffffp+600 -0x1.0000000000000fffffffffffp+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+600 -0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000100000000000p+600 +0x1.000000000000100000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+600 +0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000100000000000p+600 -0x1.000000000000100000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+600 -0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000100000000001p+600 +0x1.000000000000100000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+600 +0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000100000000001p+600 -0x1.000000000000100000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+600 -0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.00000000000017ffffffffffp+600 +0x1.00000000000017ffffffffffp+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+600 +0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.00000000000017ffffffffffp+600 -0x1.00000000000017ffffffffffp+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+600 -0x1.0000000000001p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000180000000000p+600 +0x1.000000000000180000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+600 +0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000180000000000p+600 -0x1.000000000000180000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+600 -0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000180000000001p+600 +0x1.000000000000180000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+600 +0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000180000000001p+600 -0x1.000000000000180000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+600 -0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.0000000000001fffffffffffp+600 +0x1.0000000000001fffffffffffp+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+600 +0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.0000000000001fffffffffffp+600 -0x1.0000000000001fffffffffffp+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+600 -0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000200000000000p+600 +0x1.000000000000200000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+600 +0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000200000000000p+600 -0x1.000000000000200000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+600 -0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000200000000001p+600 +0x1.000000000000200000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+600 +0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000200000000001p+600 -0x1.000000000000200000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+600 -0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.00000000000027ffffffffffp+600 +0x1.00000000000027ffffffffffp+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+600 +0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.00000000000027ffffffffffp+600 -0x1.00000000000027ffffffffffp+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+600 -0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000280000000000p+600 +0x1.000000000000280000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+600 +0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000280000000000p+600 -0x1.000000000000280000000000p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+600 -0x1.0000000000002p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000280000000001p+600 +0x1.000000000000280000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000003p+600 +0x1.0000000000003p+600))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000280000000001p+600 -0x1.000000000000280000000001p+600)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000003p+600 -0x1.0000000000003p+600))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000100000000000 +0x2000000000000100000000000)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000000p+97 +0x1.0000000000000p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000100000000000 -0x2000000000000100000000000)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000000p+97 -0x1.0000000000000p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000100000000001 +0x2000000000000100000000001)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+97 +0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000100000000001 -0x2000000000000100000000001)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+97 -0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x20000000000001fffffffffff +0x20000000000001fffffffffff)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+97 +0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x20000000000001fffffffffff -0x20000000000001fffffffffff)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+97 -0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000200000000000 +0x2000000000000200000000000)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+97 +0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000200000000000 -0x2000000000000200000000000)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+97 -0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000200000000001 +0x2000000000000200000000001)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+97 +0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000200000000001 -0x2000000000000200000000001)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+97 -0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x20000000000002fffffffffff +0x20000000000002fffffffffff)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+97 +0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x20000000000002fffffffffff -0x20000000000002fffffffffff)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+97 -0x1.0000000000001p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000300000000000 +0x2000000000000300000000000)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+97 +0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000300000000000 -0x2000000000000300000000000)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+97 -0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000300000000001 +0x2000000000000300000000001)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+97 +0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000300000000001 -0x2000000000000300000000001)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+97 -0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x20000000000003fffffffffff +0x20000000000003fffffffffff)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+97 +0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x20000000000003fffffffffff -0x20000000000003fffffffffff)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+97 -0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000400000000000 +0x2000000000000400000000000)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+97 +0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000400000000000 -0x2000000000000400000000000)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+97 -0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000400000000001 +0x2000000000000400000000001)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+97 +0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000400000000001 -0x2000000000000400000000001)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+97 -0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x20000000000004fffffffffff +0x20000000000004fffffffffff)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+97 +0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x20000000000004fffffffffff -0x20000000000004fffffffffff)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+97 -0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000500000000000 +0x2000000000000500000000000)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+97 +0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000500000000000 -0x2000000000000500000000000)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+97 -0x1.0000000000002p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +0x2000000000000500000000001 +0x2000000000000500000000001)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000003p+97 +0x1.0000000000003p+97))
(module (func (export "f") (result v128) (v128.const f64x2 -0x2000000000000500000000001 -0x2000000000000500000000001)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000003p+97 -0x1.0000000000003p+97))
(module (func (export "f") (result v128) (v128.const f64x2 +1152921504606847104 +1152921504606847104)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000000p+60 +0x1.0000000000000p+60))
(module (func (export "f") (result v128) (v128.const f64x2 -1152921504606847104 -1152921504606847104)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000000p+60 -0x1.0000000000000p+60))
(module (func (export "f") (result v128) (v128.const f64x2 +1152921504606847105 +1152921504606847105)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+60 +0x1.0000000000001p+60))
(module (func (export "f") (result v128) (v128.const f64x2 -1152921504606847105 -1152921504606847105)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+60 -0x1.0000000000001p+60))
(module (func (export "f") (result v128) (v128.const f64x2 +1152921504606847359 +1152921504606847359)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000001p+60 +0x1.0000000000001p+60))
(module (func (export "f") (result v128) (v128.const f64x2 -1152921504606847359 -1152921504606847359)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000001p+60 -0x1.0000000000001p+60))
(module (func (export "f") (result v128) (v128.const f64x2 +1152921504606847360 +1152921504606847360)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000002p+60 +0x1.0000000000002p+60))
(module (func (export "f") (result v128) (v128.const f64x2 -1152921504606847360 -1152921504606847360)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000002p+60 -0x1.0000000000002p+60))

;; f64x2, subnormal
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000080000000000p-1022 +0x0.000000000000080000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000000p-1022 +0x0.0000000000000p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000080000000000p-1022 -0x0.000000000000080000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000000p-1022 -0x0.0000000000000p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000080000000001p-1022 +0x0.000000000000080000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000001p-1022 +0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000080000000001p-1022 -0x0.000000000000080000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000001p-1022 -0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.0000000000000fffffffffffp-1022 +0x0.0000000000000fffffffffffp-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000001p-1022 +0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.0000000000000fffffffffffp-1022 -0x0.0000000000000fffffffffffp-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000001p-1022 -0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000100000000000p-1022 +0x0.000000000000100000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000001p-1022 +0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000100000000000p-1022 -0x0.000000000000100000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000001p-1022 -0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000100000000001p-1022 +0x0.000000000000100000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000001p-1022 +0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000100000000001p-1022 -0x0.000000000000100000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000001p-1022 -0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.00000000000017ffffffffffp-1022 +0x0.00000000000017ffffffffffp-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000001p-1022 +0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.00000000000017ffffffffffp-1022 -0x0.00000000000017ffffffffffp-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000001p-1022 -0x0.0000000000001p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000180000000000p-1022 +0x0.000000000000180000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000002p-1022 +0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000180000000000p-1022 -0x0.000000000000180000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000002p-1022 -0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000180000000001p-1022 +0x0.000000000000180000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000002p-1022 +0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000180000000001p-1022 -0x0.000000000000180000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000002p-1022 -0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.0000000000001fffffffffffp-1022 +0x0.0000000000001fffffffffffp-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000002p-1022 +0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.0000000000001fffffffffffp-1022 -0x0.0000000000001fffffffffffp-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000002p-1022 -0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000200000000000p-1022 +0x0.000000000000200000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000002p-1022 +0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000200000000000p-1022 -0x0.000000000000200000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000002p-1022 -0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000200000000001p-1022 +0x0.000000000000200000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000002p-1022 +0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000200000000001p-1022 -0x0.000000000000200000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000002p-1022 -0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.00000000000027ffffffffffp-1022 +0x0.00000000000027ffffffffffp-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000002p-1022 +0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.00000000000027ffffffffffp-1022 -0x0.00000000000027ffffffffffp-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000002p-1022 -0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x0.000000000000280000000000p-1022 +0x0.000000000000280000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x0.0000000000002p-1022 +0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x0.000000000000280000000000p-1022 -0x0.000000000000280000000000p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x0.0000000000002p-1022 -0x0.0000000000002p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.000000000000280000000001p-1022 +0x1.000000000000280000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.0000000000003p-1022 +0x1.0000000000003p-1022))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.000000000000280000000001p-1022 -0x1.000000000000280000000001p-1022)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.0000000000003p-1022 -0x1.0000000000003p-1022))

;; f64x2, round down at limit to infinity
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.fffffffffffff4p1023 +0x1.fffffffffffff4p1023)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.fffffffffffffp1023 +0x1.fffffffffffffp1023))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.fffffffffffff4p1023 -0x1.fffffffffffff4p1023)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.fffffffffffffp1023 -0x1.fffffffffffffp1023))
(module (func (export "f") (result v128) (v128.const f64x2 +0x1.fffffffffffff7ffffffp1023 +0x1.fffffffffffff7ffffffp1023)))
(assert_return (invoke "f") (v128.const f64x2 +0x1.fffffffffffffp1023 +0x1.fffffffffffffp1023))
(module (func (export "f") (result v128) (v128.const f64x2 -0x1.fffffffffffff7ffffffp1023 -0x1.fffffffffffff7ffffffp1023)))
(assert_return (invoke "f") (v128.const f64x2 -0x1.fffffffffffffp1023 -0x1.fffffffffffffp1023))

;; As parameters of control constructs

(module (memory 1)
  (func (export "as-br-retval") (result v128)
    (block (result v128) (br 0 (v128.const i32x4 0x03020100 0x07060504 0x0b0a0908 0x0f0e0d0c)))
  )
  (func (export "as-br_if-retval") (result v128)
    (block (result v128)
      (br_if 0 (v128.const i32x4 0 1 2 3) (i32.const 1))
    )
  )
  (func (export "as-return-retval") (result v128)
    (return (v128.const i32x4 0 1 2 3))
  )
  (func (export "as-if-then-retval") (result v128)
    (if (result v128) (i32.const 1)
      (then (v128.const i32x4 0 1 2 3)) (else (v128.const i32x4 3 2 1 0))
    )
  )
  (func (export "as-if-else-retval") (result v128)
    (if (result v128) (i32.const 0)
      (then (v128.const i32x4 0 1 2 3)) (else (v128.const i32x4 3 2 1 0))
    )
  )
  (func $f (param v128 v128 v128) (result v128) (v128.const i32x4 0 1 2 3))
  (func (export "as-call-param") (result v128)
    (call $f (v128.const i32x4 0 1 2 3) (v128.const i32x4 0 1 2 3) (v128.const i32x4 0 1 2 3))
  )
  (func (export "as-block-retval") (result v128)
    (block (result v128) (v128.const i32x4 0 1 2 3))
  )
  (func (export "as-loop-retval") (result v128)
    (loop (result v128) (v128.const i32x4 0 1 2 3))
  )
  (func (export "as-drop-operand")
    (drop (v128.const i32x4 0 1 2 3))
  )

  (func (export "as-br-retval2") (result v128)
    (block (result v128) (br 0 (v128.const i64x2 0x0302010007060504 0x0b0a09080f0e0d0c)))
  )
  (func (export "as-br_if-retval2") (result v128)
    (block (result v128)
      (br_if 0 (v128.const i64x2 0 1) (i32.const 1))
    )
  )
  (func (export "as-return-retval2") (result v128)
    (return (v128.const i64x2 0 1))
  )
  (func (export "as-if-then-retval2") (result v128)
    (if (result v128) (i32.const 1)
      (then (v128.const i64x2 0 1)) (else (v128.const i64x2 1 0))
    )
  )
  (func (export "as-if-else-retval2") (result v128)
    (if (result v128) (i32.const 0)
      (then (v128.const i64x2 0 1)) (else (v128.const i64x2 1 0))
    )
  )
  (func $f2 (param v128 v128 v128) (result v128) (v128.const i64x2 0 1))
  (func (export "as-call-param2") (result v128)
    (call $f2 (v128.const i64x2 0 1) (v128.const i64x2 0 1) (v128.const i64x2 0 1))
  )

  (type $sig (func (param v128 v128 v128) (result v128)))
  (table funcref (elem $f $f2))
  (func (export "as-call_indirect-param") (result v128)
    (call_indirect (type $sig)
      (v128.const i32x4 0 1 2 3) (v128.const i32x4 0 1 2 3) (v128.const i32x4 0 1 2 3) (i32.const 0)
    )
  )
  (func (export "as-call_indirect-param2") (result v128)
    (call_indirect (type $sig)
      (v128.const i64x2 0 1) (v128.const i64x2 0 1) (v128.const i64x2 0 1) (i32.const 1)
    )
  )
  (func (export "as-block-retval2") (result v128)
    (block (result v128) (v128.const i64x2 0 1))
  )
  (func (export "as-loop-retval2") (result v128)
    (loop (result v128) (v128.const i64x2 0 1))
  )
  (func (export "as-drop-operand2")
    (drop (v128.const i64x2 0 1))
  )
)

(assert_return (invoke "as-br-retval") (v128.const i32x4 0x03020100 0x07060504 0x0b0a0908 0x0f0e0d0c))
(assert_return (invoke "as-br_if-retval") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "as-return-retval") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "as-if-then-retval") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "as-if-else-retval") (v128.const i32x4 3 2 1 0))
(assert_return (invoke "as-call-param") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "as-call_indirect-param") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "as-block-retval") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "as-loop-retval") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "as-drop-operand"))

(assert_return (invoke "as-br-retval2") (v128.const i64x2 0x0302010007060504 0x0b0a09080f0e0d0c))
(assert_return (invoke "as-br_if-retval2") (v128.const i64x2 0 1))
(assert_return (invoke "as-return-retval2") (v128.const i64x2 0 1))
(assert_return (invoke "as-if-then-retval2") (v128.const i64x2 0 1))
(assert_return (invoke "as-if-else-retval2") (v128.const i64x2 1 0))
(assert_return (invoke "as-call-param2") (v128.const i64x2 0 1))
(assert_return (invoke "as-call_indirect-param2") (v128.const i64x2 0 1))
(assert_return (invoke "as-block-retval2") (v128.const i64x2 0 1))
(assert_return (invoke "as-loop-retval2") (v128.const i64x2 0 1))
(assert_return (invoke "as-drop-operand2"))

;; v128 locals

(module (memory 1)
  (func (export "as-local.set/get-value_0_0") (param $0 v128) (result v128)
    (local v128 v128 v128 v128)
    (local.set 0 (local.get $0))
    (local.get 0)
  )
  (func (export "as-local.set/get-value_0_1") (param $0 v128) (result v128)
    (local v128 v128 v128 v128)
    (local.set 0 (local.get $0))
    (local.set 1 (local.get 0))
    (local.set 2 (local.get 1))
    (local.set 3 (local.get 2))
    (local.get 0)
  )
  (func (export "as-local.set/get-value_3_0") (param $0 v128) (result v128)
    (local v128 v128 v128 v128)
    (local.set 0 (local.get $0))
    (local.set 1 (local.get 0))
    (local.set 2 (local.get 1))
    (local.set 3 (local.get 2))
    (local.get 3)
  )
  (func (export "as-local.tee-value") (result v128)
    (local v128)
    (local.tee 0 (v128.const i32x4 0 1 2 3))
  )
)

(assert_return (invoke "as-local.set/get-value_0_0" (v128.const i32x4 0 0 0 0)) (v128.const i32x4 0 0 0 0))
(assert_return (invoke "as-local.set/get-value_0_1" (v128.const i32x4 1 1 1 1)) (v128.const i32x4 1 1 1 1))
(assert_return (invoke "as-local.set/get-value_3_0" (v128.const i32x4 2 2 2 2)) (v128.const i32x4 2 2 2 2))
(assert_return (invoke "as-local.tee-value") (v128.const i32x4 0 1 2 3))


;; v128 globals

(module (memory 1)
  (global $g0 (mut v128) (v128.const i32x4 0 1 2 3))
  (global $g1 (mut v128) (v128.const i32x4 4 5 6 7))
  (global $g2 (mut v128) (v128.const i32x4 8 9 10 11))
  (global $g3 (mut v128) (v128.const i32x4 12 13 14 15))
  (global $g4 (mut v128) (v128.const i32x4 16 17 18 19))

  (func $set_g0 (export "as-global.set_value_$g0") (param $0 v128)
    (global.set $g0 (local.get $0))
  )
  (func $set_g1_g2 (export "as-global.set_value_$g1_$g2") (param $0 v128) (param $1 v128)
    (global.set $g1 (local.get $0))
    (global.set $g2 (local.get $1))
  )
  (func $set_g0_g1_g2_g3 (export "as-global.set_value_$g0_$g1_$g2_$g3") (param $0 v128) (param $1 v128) (param $2 v128) (param $3 v128)
    (call $set_g0 (local.get $0))
    (call $set_g1_g2 (local.get $1) (local.get $2))
    (global.set $g3 (local.get $3))
  )
  (func (export "global.get_g0") (result v128)
    (global.get $g0)
  )
  (func (export "global.get_g1") (result v128)
    (global.get $g1)
  )
  (func (export "global.get_g2") (result v128)
    (global.get $g2)
  )
  (func (export "global.get_g3") (result v128)
    (global.get $g3)
  )
)

(assert_return (invoke "as-global.set_value_$g0_$g1_$g2_$g3" (v128.const i32x4 1 1 1 1)
                                                             (v128.const i32x4 2 2 2 2)
                                                             (v128.const i32x4 3 3 3 3)
                                                             (v128.const i32x4 4 4 4 4)))
(assert_return (invoke "global.get_g0") (v128.const i32x4 1 1 1 1))
(assert_return (invoke "global.get_g1") (v128.const i32x4 2 2 2 2))
(assert_return (invoke "global.get_g2") (v128.const i32x4 3 3 3 3))
(assert_return (invoke "global.get_g3") (v128.const i32x4 4 4 4 4))


;; Test integer literal parsing.

(module
  (func (export "i32x4.test") (result v128) (return (v128.const i32x4 0x0bAdD00D 0x0bAdD00D 0x0bAdD00D 0x0bAdD00D)))
  (func (export "i32x4.smax") (result v128) (return (v128.const i32x4 0x7fffffff 0x7fffffff 0x7fffffff 0x7fffffff)))
  (func (export "i32x4.neg_smax") (result v128) (return (v128.const i32x4 -0x7fffffff -0x7fffffff -0x7fffffff -0x7fffffff)))
  (func (export "i32x4.inc_smin") (result v128) (return (i32x4.add (v128.const i32x4 -0x80000000 -0x80000000 -0x80000000 -0x80000000) (v128.const i32x4 1 1 1 1))))
  (func (export "i32x4.neg_zero") (result v128) (return (v128.const i32x4 -0x0 -0x0 -0x0 -0x0)))
  (func (export "i32x4.not_octal") (result v128) (return (v128.const i32x4 010 010 010 010)))
  (func (export "i32x4.plus_sign") (result v128) (return (v128.const i32x4 +42 +42 +42 +42)))

  (func (export "i32x4-dec-sep1") (result v128) (v128.const i32x4 1_000_000 1_000_000 1_000_000 1_000_000))
  (func (export "i32x4-dec-sep2") (result v128) (v128.const i32x4 1_0_0_0 1_0_0_0 1_0_0_0 1_0_0_0))
  (func (export "i32x4-hex-sep1") (result v128) (v128.const i32x4 0xa_0f_00_99 0xa_0f_00_99 0xa_0f_00_99 0xa_0f_00_99))
  (func (export "i32x4-hex-sep2") (result v128) (v128.const i32x4 0x1_a_A_0_f 0x1_a_A_0_f 0x1_a_A_0_f 0x1_a_A_0_f))

  (func (export "i64x2.test") (result v128) (return (v128.const i64x2 0x0bAdD00D0bAdD00D 0x0bAdD00D0bAdD00D)))
  (func (export "i64x2.smax") (result v128) (return (v128.const i64x2 0x7fffffffffffffff 0x7fffffffffffffff)))
  (func (export "i64x2.neg_smax") (result v128) (return (v128.const i64x2 -0x7fffffffffffffff -0x7fffffffffffffff)))
  (func (export "i64x2.inc_smin") (result v128) (return (i64x2.add (v128.const i64x2 -0x8000000000000000 -0x8000000000000000) (v128.const i64x2 1 1))))
  (func (export "i64x2.neg_zero") (result v128) (return (v128.const i64x2 -0x0 -0x0)))
  (func (export "i64x2.not_octal") (result v128) (return (v128.const i64x2 010010 010010)))
  (func (export "i64x2.plus_sign") (result v128) (return (v128.const i64x2 +42 +42)))

  (func (export "i64x2-dec-sep1") (result v128) (v128.const i64x2 10_000_000_000_000 10_000_000_000_000))
  (func (export "i64x2-dec-sep2") (result v128) (v128.const i64x2 1_0_0_0_0_0_0_0 1_0_0_0_0_0_0_0))
  (func (export "i64x2-hex-sep1") (result v128) (v128.const i64x2 0xa_0f_00_99_0a_0f_00_99 0xa_0f_00_99_0a_0f_00_99))
  (func (export "i64x2-hex-sep2") (result v128) (v128.const i64x2 0x1_a_A_0_f_1_a_A_0_f 0x1_a_A_0_f_1_a_A_0_f))
)

(assert_return (invoke "i32x4.test") (v128.const i32x4 195940365 195940365 195940365 195940365))
(assert_return (invoke "i32x4.smax") (v128.const i32x4 2147483647 2147483647 2147483647 2147483647))
(assert_return (invoke "i32x4.neg_smax") (v128.const i32x4 -2147483647 -2147483647 -2147483647 -2147483647))
(assert_return (invoke "i32x4.inc_smin") (v128.const i32x4 -2147483647 -2147483647 -2147483647 -2147483647))
(assert_return (invoke "i32x4.neg_zero") (v128.const i32x4 0 0 0 0))
(assert_return (invoke "i32x4.not_octal") (v128.const i32x4 10 10 10 10))
(assert_return (invoke "i32x4.plus_sign") (v128.const i32x4 42 42 42 42))

(assert_return (invoke "i32x4-dec-sep1") (v128.const i32x4 1000000 1000000 1000000 1000000))
(assert_return (invoke "i32x4-dec-sep2") (v128.const i32x4 1000 1000 1000 1000))
(assert_return (invoke "i32x4-hex-sep1") (v128.const i32x4 0xa0f0099 0xa0f0099 0xa0f0099 0xa0f0099))
(assert_return (invoke "i32x4-hex-sep2") (v128.const i32x4 0x1aa0f 0x1aa0f 0x1aa0f 0x1aa0f))

(assert_return (invoke "i64x2.test") (v128.const i64x2 841557459837243405 841557459837243405))
(assert_return (invoke "i64x2.smax") (v128.const i64x2 9223372036854775807 9223372036854775807))
(assert_return (invoke "i64x2.neg_smax") (v128.const i64x2 -9223372036854775807 -9223372036854775807))
(assert_return (invoke "i64x2.inc_smin") (v128.const i64x2 -9223372036854775807 -9223372036854775807))
(assert_return (invoke "i64x2.neg_zero") (v128.const i64x2 0 0))
(assert_return (invoke "i64x2.not_octal") (v128.const i64x2 10010 10010))
(assert_return (invoke "i64x2.plus_sign") (v128.const i64x2 42 42))

(assert_return (invoke "i64x2-dec-sep1") (v128.const i64x2 10000000000000 10000000000000))
(assert_return (invoke "i64x2-dec-sep2") (v128.const i64x2 10000000 10000000))
(assert_return (invoke "i64x2-hex-sep1") (v128.const i64x2 0xa0f00990a0f0099 0xa0f00990a0f0099))
(assert_return (invoke "i64x2-hex-sep2") (v128.const i64x2 0x1aa0f1aa0f 0x1aa0f1aa0f))

(assert_malformed
  (module quote "(global v128 (v128.const i32x4 _100 _100 _100 _100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 +_100 +_100 +_100 +_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 -_100 -_100 -_100 -_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 99_ 99_ 99_ 99_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 1__000 1__000 1__000 1__000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 _0x100 _0x100 _0x100 _0x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 0_x100 0_x100 0_x100 0_x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 0x_100 0x_100 0x_100 0x_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 0x00_ 0x00_ 0x00_ 0x00_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i32x4 0xff__ffff 0xff__ffff 0xff__ffff 0xff__ffff))")
  "unknown operator"
)

(assert_malformed
  (module quote "(global v128 (v128.const i64x2 _100_100 _100_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 +_100_100 +_100_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 -_100_100 -_100_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 99_99_ 99_99_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 1__000_000 1__000_000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 _0x100000 _0x100000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 0_x100000 0_x100000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 0x_100000 0x_100000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 0x00_ 0x00_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const i64x2 0xff__ffff_ffff_ffff 0xff__ffff_ffff_ffff))")
  "unknown operator"
)

;; Test floating-point literal parsing.

(module
  (func (export "f32-dec-sep1") (result v128) (v128.const f32x4 1_000_000 1_000_000 1_000_000 1_000_000))
  (func (export "f32-dec-sep2") (result v128) (v128.const f32x4 1_0_0_0 1_0_0_0 1_0_0_0 1_0_0_0))
  (func (export "f32-dec-sep3") (result v128) (v128.const f32x4 100_3.141_592 100_3.141_592 100_3.141_592 100_3.141_592))
  (func (export "f32-dec-sep4") (result v128) (v128.const f32x4 99e+1_3 99e+1_3 99e+1_3 99e+1_3))
  (func (export "f32-dec-sep5") (result v128) (v128.const f32x4 122_000.11_3_54E0_2_3 122_000.11_3_54E0_2_3 122_000.11_3_54E0_2_3 122_000.11_3_54E0_2_3))
  (func (export "f32-hex-sep1") (result v128) (v128.const f32x4 0xa_0f_00_99 0xa_0f_00_99 0xa_0f_00_99 0xa_0f_00_99))
  (func (export "f32-hex-sep2") (result v128) (v128.const f32x4 0x1_a_A_0_f 0x1_a_A_0_f 0x1_a_A_0_f 0x1_a_A_0_f))
  (func (export "f32-hex-sep3") (result v128) (v128.const f32x4 0xa0_ff.f141_a59a 0xa0_ff.f141_a59a 0xa0_ff.f141_a59a 0xa0_ff.f141_a59a))
  (func (export "f32-hex-sep4") (result v128) (v128.const f32x4 0xf0P+1_3 0xf0P+1_3 0xf0P+1_3 0xf0P+1_3))
  (func (export "f32-hex-sep5") (result v128) (v128.const f32x4 0x2a_f00a.1f_3_eep2_3 0x2a_f00a.1f_3_eep2_3 0x2a_f00a.1f_3_eep2_3 0x2a_f00a.1f_3_eep2_3))
  (func (export "f64-dec-sep1") (result v128) (v128.const f64x2 1_000_000 1_000_000))
  (func (export "f64-dec-sep2") (result v128) (v128.const f64x2 1_0_0_0 1_0_0_0))
  (func (export "f64-dec-sep3") (result v128) (v128.const f64x2 100_3.141_592 100_3.141_592))
  (func (export "f64-dec-sep4") (result v128) (v128.const f64x2 99e+1_3 99e+1_3))
  (func (export "f64-dec-sep5") (result v128) (v128.const f64x2 122_000.11_3_54E0_2_3 122_000.11_3_54E0_2_3))
  (func (export "f64-hex-sep1") (result v128) (v128.const f64x2 0xa_0f_00_99 0xa_0f_00_99))
  (func (export "f64-hex-sep2") (result v128) (v128.const f64x2 0x1_a_A_0_f 0x1_a_A_0_f))
  (func (export "f64-hex-sep3") (result v128) (v128.const f64x2 0xa0_ff.f141_a59a 0xa0_ff.f141_a59a))
  (func (export "f64-hex-sep4") (result v128) (v128.const f64x2 0xf0P+1_3 0xf0P+1_3))
  (func (export "f64-hex-sep5") (result v128) (v128.const f64x2 0x2a_f00a.1f_3_eep2_3 0x2a_f00a.1f_3_eep2_3))
)

(assert_return (invoke "f32-dec-sep1") (v128.const f32x4 1000000 1000000 1000000 1000000))
(assert_return (invoke "f32-dec-sep2") (v128.const f32x4 1000 1000 1000 1000))
(assert_return (invoke "f32-dec-sep3") (v128.const f32x4 1003.141592 1003.141592 1003.141592 1003.141592))
(assert_return (invoke "f32-dec-sep4") (v128.const f32x4 99e+13 99e+13 99e+13 99e+13))
(assert_return (invoke "f32-dec-sep5") (v128.const f32x4 122000.11354e23 122000.11354e23 122000.11354e23 122000.11354e23))
(assert_return (invoke "f32-hex-sep1") (v128.const f32x4 0xa0f0099 0xa0f0099 0xa0f0099 0xa0f0099))
(assert_return (invoke "f32-hex-sep2") (v128.const f32x4 0x1aa0f 0x1aa0f 0x1aa0f 0x1aa0f))
(assert_return (invoke "f32-hex-sep3") (v128.const f32x4 0xa0ff.f141a59a 0xa0ff.f141a59a 0xa0ff.f141a59a 0xa0ff.f141a59a))
(assert_return (invoke "f32-hex-sep4") (v128.const f32x4 0xf0P+13 0xf0P+13 0xf0P+13 0xf0P+13))
(assert_return (invoke "f32-hex-sep5") (v128.const f32x4 0x2af00a.1f3eep23 0x2af00a.1f3eep23 0x2af00a.1f3eep23 0x2af00a.1f3eep23))
(assert_return (invoke "f64-dec-sep1") (v128.const f64x2 1000000 1000000))
(assert_return (invoke "f64-dec-sep2") (v128.const f64x2 1000 1000))
(assert_return (invoke "f64-dec-sep3") (v128.const f64x2 1003.141592 1003.141592))
(assert_return (invoke "f64-dec-sep4") (v128.const f64x2 99e+13 99e+13))
(assert_return (invoke "f64-dec-sep5") (v128.const f64x2 122000.11354e23 122000.11354e23))
(assert_return (invoke "f64-hex-sep1") (v128.const f64x2 0xa0f0099 0xa0f0099))
(assert_return (invoke "f64-hex-sep2") (v128.const f64x2 0x1aa0f 0x1aa0f))
(assert_return (invoke "f64-hex-sep3") (v128.const f64x2 0xa0ff.f141a59a 0xa0ff.f141a59a))
(assert_return (invoke "f64-hex-sep4") (v128.const f64x2 0xf0P+13 0xf0P+13))
(assert_return (invoke "f64-hex-sep5") (v128.const f64x2 0x2af00a.1f3eep23 0x2af00a.1f3eep23))

(assert_malformed
  (module quote "(global v128 (v128.const f32x4 _100 _100 _100 _100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 +_100 +_100 +_100 +_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 -_100 -_100 -_100 -_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 99_ 99_ 99_ 99_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1__000 1__000 1__000 1__000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 _1.0 _1.0 _1.0 _1.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1.0_ 1.0_ 1.0_ 1.0_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1_.0 1_.0 1_.0 1_.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1._0 1._0 1._0 1._0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 _1e1 _1e1 _1e1 _1e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1e1_ 1e1_ 1e1_ 1e1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1_e1 1_e1 1_e1 1_e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1e_1 1e_1 1e_1 1e_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 _1.0e1 _1.0e1 _1.0e1 _1.0e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1.0e1_ 1.0e1_ 1.0e1_ 1.0e1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1.0_e1 1.0_e1 1.0_e1 1.0_e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1.0e_1 1.0e_1 1.0e_1 1.0e_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1.0e+_1 1.0e+_1 1.0e+_1 1.0e+_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 1.0e_+1 1.0e_+1 1.0e_+1 1.0e_+1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 _0x100 _0x100 _0x100 _0x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0_x100 0_x100 0_x100 0_x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x_100 0x_100 0x_100 0x_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x00_ 0x00_ 0x00_ 0x00_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0xff__ffff 0xff__ffff 0xff__ffff 0xff__ffff))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x_1.0 0x_1.0 0x_1.0 0x_1.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1.0_ 0x1.0_ 0x1.0_ 0x1.0_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1_.0 0x1_.0 0x1_.0 0x1_.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1._0 0x1._0 0x1._0 0x1._0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x_1p1 0x_1p1 0x_1p1 0x_1p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1p1_ 0x1p1_ 0x1p1_ 0x1p1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1_p1 0x1_p1 0x1_p1 0x1_p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1p_1 0x1p_1 0x1p_1 0x1p_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x_1.0p1 0x_1.0p1 0x_1.0p1 0x_1.0p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1.0p1_ 0x1.0p1_ 0x1.0p1_ 0x1.0p1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1.0_p1 0x1.0_p1 0x1.0_p1 0x1.0_p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1.0p_1 0x1.0p_1 0x1.0p_1 0x1.0p_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1.0p+_1 0x1.0p+_1 0x1.0p+_1 0x1.0p+_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f32x4 0x1.0p_+1 0x1.0p_+1 0x1.0p_+1 0x1.0p_+1))")
  "unknown operator"
)

(assert_malformed
  (module quote "(global v128 (v128.const f64x2 _100 _100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 +_100 +_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 -_100 -_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 99_ 99_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1__000 1__000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 _1.0 _1.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1.0_ 1.0_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1_.0 1_.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1._0 1._0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 _1e1 _1e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1e1_ 1e1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1_e1 1_e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1e_1 1e_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 _1.0e1 _1.0e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1.0e1_ 1.0e1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1.0_e1 1.0_e1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1.0e_1 1.0e_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1.0e+_1 1.0e+_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 1.0e_+1 1.0e_+1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 _0x100 _0x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0_x100 0_x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x_100 0x_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x00_ 0x00_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0xff__ffff 0xff__ffff))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x_1.0 0x_1.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1.0_ 0x1.0_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1_.0 0x1_.0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1._0 0x1._0))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x_1p1 0x_1p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1p1_ 0x1p1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1_p1 0x1_p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1p_1 0x1p_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x_1.0p1 0x_1.0p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1.0p1_ 0x1.0p1_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1.0_p1 0x1.0_p1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1.0p_1 0x1.0p_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1.0p+_1 0x1.0p+_1))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global v128 (v128.const f64x2 0x1.0p_+1 0x1.0p_+1))")
  "unknown operator"
)

;; Test parsing an integer from binary

(module binary
  "\00asm" "\01\00\00\00"
  "\01\05\01"                                ;; type   section
  "\60\00\01\7b"                             ;; type 0 (func)
  "\03\02\01\00"                             ;; func   section
  "\07\0f\01\0b"                             ;; export section
  "\70\61\72\73\65\5f\69\38\78\31\36\00\00"  ;; export name (parse_i8x16)
  "\0a\16\01"                                ;; code   section
  "\14\00\fd\0c"                             ;; func body
  "\00\00\00\00"                             ;; data lane 0~3   (0,    0,    0,    0)
  "\80\80\80\80"                             ;; data lane 4~7   (-128, -128, -128, -128)
  "\ff\ff\ff\ff"                             ;; data lane 8~11  (0xff, 0xff, 0xff, 0xff)
  "\ff\ff\ff\ff"                             ;; data lane 12~15 (255,  255,  255,  255)
  "\0b"                                      ;; end
)
(assert_return (invoke "parse_i8x16") (v128.const i8x16 0 0 0 0 -128 -128 -128 -128 0xff 0xff 0xff 0xff 255 255 255 255))

(module binary
  "\00asm" "\01\00\00\00"
  "\01\05\01"                                ;; type   section
  "\60\00\01\7b"                             ;; type 0 (func)
  "\03\02\01\00"                             ;; func   section
  "\07\0f\01\0b"                             ;; export section
  "\70\61\72\73\65\5f\69\31\36\78\38\00\00"  ;; export name (parse_i16x8)
  "\0a\16\01"                                ;; code   section
  "\14\00\fd\0c"                             ;; func body
  "\00\00\00\00"                             ;; data lane 0, 1 (0,      0)
  "\00\80\00\80"                             ;; data lane 2, 3 (-32768, -32768)
  "\ff\ff\ff\ff"                             ;; data lane 4, 5 (65535,  65535)
  "\ff\ff\ff\ff"                             ;; data lane 6, 7 (0xffff, 0xffff)
  "\0b"                                      ;; end
)
(assert_return (invoke "parse_i16x8") (v128.const i16x8 0 0 -32768 -32768 65535 65535 0xffff 0xffff))

(module binary
  "\00asm" "\01\00\00\00"
  "\01\05\01"                                ;; type   section
  "\60\00\01\7b"                             ;; type 0 (func)
  "\03\02\01\00"                             ;; func   section
  "\07\0f\01\0b"                             ;; export section
  "\70\61\72\73\65\5f\69\33\32\78\34\00\00"  ;; export name (parse_i32x4)
  "\0a\16\01"                                ;; code   section
  "\14\00\fd\0c"                             ;; func body
  "\d1\ff\ff\ff"                             ;; data lane 0 (4294967249)
  "\d1\ff\ff\ff"                             ;; data lane 1 (4294967249)
  "\d1\ff\ff\ff"                             ;; data lane 2 (4294967249)
  "\d1\ff\ff\ff"                             ;; data lane 3 (4294967249)
  "\0b"                                      ;; end
)
(assert_return (invoke "parse_i32x4") (v128.const i32x4 4294967249 4294967249 4294967249 4294967249))

(module binary
  "\00asm" "\01\00\00\00"
  "\01\05\01"                                ;; type   section
  "\60\00\01\7b"                             ;; type 0 (func)
  "\03\02\01\00"                             ;; func   section
  "\07\0f\01\0b"                             ;; export section
  "\70\61\72\73\65\5f\69\36\34\78\32\00\00"  ;; export name (parse_i64x2)
  "\0a\16\01"                                ;; code   section
  "\14\00\fd\0c"                             ;; func body
  "\ff\ff\ff\ff\ff\ff\ff\7f"                 ;; data lane 0 (9223372036854775807)
  "\ff\ff\ff\ff\ff\ff\ff\7f"                 ;; data lane 1 (9223372036854775807)
  "\0b"                                      ;; end
)
(assert_return (invoke "parse_i64x2") (v128.const i64x2 9223372036854775807 9223372036854775807))

;; Test parsing a float from binary

(module binary
  "\00asm" "\01\00\00\00"
  "\01\05\01"                                ;; type   section
  "\60\00\01\7b"                             ;; type 0 (func)
  "\03\02\01\00"                             ;; func   section
  "\07\0f\01\0b"                             ;; export section
  "\70\61\72\73\65\5f\66\33\32\78\34\00\00"  ;; export name (parse_f32x4)
  "\0a\16\01"                                ;; code   section
  "\14\00\fd\0c"                             ;; func body
  "\00\00\80\4f"                             ;; data lane 0 (4294967249)
  "\00\00\80\4f"                             ;; data lane 1 (4294967249)
  "\00\00\80\4f"                             ;; data lane 2 (4294967249)
  "\00\00\80\4f"                             ;; data lane 3 (4294967249)
  "\0b"                                      ;; end
)
(assert_return (invoke "parse_f32x4") (v128.const f32x4 4294967249 4294967249 4294967249 4294967249))

(module binary
  "\00asm" "\01\00\00\00"
  "\01\05\01"                                ;; type   section
  "\60\00\01\7b"                             ;; type 0 (func)
  "\03\02\01\00"                             ;; func   section
  "\07\0f\01\0b"                             ;; export section
  "\70\61\72\73\65\5f\66\36\34\78\32\00\00"  ;; export name (parse_f64x2)
  "\0a\16\01"                                ;; code   section
  "\14\00\fd\0c"                             ;; func body
  "\ff\ff\ff\ff\ff\ff\ef\7f"                 ;; data lane 0 (0x1.fffffffffffffp+1023)
  "\ff\ff\ff\ff\ff\ff\ef\7f"                 ;; data lane 1 (0x1.fffffffffffffp+1023)
  "\0b"                                      ;; end
)
(assert_return (invoke "parse_f64x2") (v128.const f64x2 0x1.fffffffffffffp+1023 0x1.fffffffffffffp+1023))
