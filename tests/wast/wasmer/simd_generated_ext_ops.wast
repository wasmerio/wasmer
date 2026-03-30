;; Generated tests for SIMD extended ops using sequential vectors.

(module
  (func (export "i16x8.extadd_pairwise_i8x16_s") (param v128) (result v128)
    (i16x8.extadd_pairwise_i8x16_s (local.get 0)))
  (func (export "i16x8.extadd_pairwise_i8x16_u") (param v128) (result v128)
    (i16x8.extadd_pairwise_i8x16_u (local.get 0)))
  (func (export "i32x4.extadd_pairwise_i16x8_s") (param v128) (result v128)
    (i32x4.extadd_pairwise_i16x8_s (local.get 0)))
  (func (export "i32x4.extadd_pairwise_i16x8_u") (param v128) (result v128)
    (i32x4.extadd_pairwise_i16x8_u (local.get 0)))

  (func (export "i16x8.extmul_low_i8x16_s") (param v128 v128) (result v128)
    (i16x8.extmul_low_i8x16_s (local.get 0) (local.get 1)))
  (func (export "i16x8.extmul_low_i8x16_u") (param v128 v128) (result v128)
    (i16x8.extmul_low_i8x16_u (local.get 0) (local.get 1)))
  (func (export "i16x8.extmul_high_i8x16_s") (param v128 v128) (result v128)
    (i16x8.extmul_high_i8x16_s (local.get 0) (local.get 1)))
  (func (export "i16x8.extmul_high_i8x16_u") (param v128 v128) (result v128)
    (i16x8.extmul_high_i8x16_u (local.get 0) (local.get 1)))

  (func (export "i32x4.extmul_low_i16x8_s") (param v128 v128) (result v128)
    (i32x4.extmul_low_i16x8_s (local.get 0) (local.get 1)))
  (func (export "i32x4.extmul_low_i16x8_u") (param v128 v128) (result v128)
    (i32x4.extmul_low_i16x8_u (local.get 0) (local.get 1)))
  (func (export "i32x4.extmul_high_i16x8_s") (param v128 v128) (result v128)
    (i32x4.extmul_high_i16x8_s (local.get 0) (local.get 1)))
  (func (export "i32x4.extmul_high_i16x8_u") (param v128 v128) (result v128)
    (i32x4.extmul_high_i16x8_u (local.get 0) (local.get 1)))

  (func (export "i64x2.extmul_low_i32x4_s") (param v128 v128) (result v128)
    (i64x2.extmul_low_i32x4_s (local.get 0) (local.get 1)))
  (func (export "i64x2.extmul_low_i32x4_u") (param v128 v128) (result v128)
    (i64x2.extmul_low_i32x4_u (local.get 0) (local.get 1)))
  (func (export "i64x2.extmul_high_i32x4_s") (param v128 v128) (result v128)
    (i64x2.extmul_high_i32x4_s (local.get 0) (local.get 1)))
  (func (export "i64x2.extmul_high_i32x4_u") (param v128 v128) (result v128)
    (i64x2.extmul_high_i32x4_u (local.get 0) (local.get 1)))

  (func (export "i32x4.dot_i16x8_s") (param v128 v128) (result v128)
    (i32x4.dot_i16x8_s (local.get 0) (local.get 1)))

  (func (export "i16x8.extend_low_i8x16_s") (param v128) (result v128)
    (i16x8.extend_low_i8x16_s (local.get 0)))
  (func (export "i16x8.extend_low_i8x16_u") (param v128) (result v128)
    (i16x8.extend_low_i8x16_u (local.get 0)))
  (func (export "i16x8.extend_high_i8x16_u") (param v128) (result v128)
    (i16x8.extend_high_i8x16_u (local.get 0)))

  (func (export "i32x4.extend_low_i16x8_s") (param v128) (result v128)
    (i32x4.extend_low_i16x8_s (local.get 0)))
  (func (export "i32x4.extend_high_i16x8_s") (param v128) (result v128)
    (i32x4.extend_high_i16x8_s (local.get 0)))
  (func (export "i32x4.extend_low_i16x8_u") (param v128) (result v128)
    (i32x4.extend_low_i16x8_u (local.get 0)))
  (func (export "i32x4.extend_high_i16x8_u") (param v128) (result v128)
    (i32x4.extend_high_i16x8_u (local.get 0)))

  (func (export "i64x2.extend_low_i32x4_u") (param v128) (result v128)
    (i64x2.extend_low_i32x4_u (local.get 0)))
  (func (export "i64x2.extend_low_i32x4_s") (param v128) (result v128)
    (i64x2.extend_low_i32x4_s (local.get 0)))
  (func (export "i64x2.extend_high_i32x4_u") (param v128) (result v128)
    (i64x2.extend_high_i32x4_u (local.get 0)))
  (func (export "i64x2.extend_high_i32x4_s") (param v128) (result v128)
    (i64x2.extend_high_i32x4_s (local.get 0)))

  (func (export "i8x16.narrow_i16x8_s") (param v128 v128) (result v128)
    (i8x16.narrow_i16x8_s (local.get 0) (local.get 1)))
  (func (export "i8x16.narrow_i16x8_u") (param v128 v128) (result v128)
    (i8x16.narrow_i16x8_u (local.get 0) (local.get 1)))
  (func (export "i16x8.narrow_i32x4_s") (param v128 v128) (result v128)
    (i16x8.narrow_i32x4_s (local.get 0) (local.get 1)))
  (func (export "i16x8.narrow_i32x4_u") (param v128 v128) (result v128)
    (i16x8.narrow_i32x4_u (local.get 0) (local.get 1)))

  (func (export "i32x4.trunc_sat_f64x2_s_zero") (param v128) (result v128)
    (i32x4.trunc_sat_f64x2_s_zero (local.get 0)))

  (func (export "f64x2.convert_low_i32x4_s") (param v128) (result v128)
    (f64x2.convert_low_i32x4_s (local.get 0)))
  (func (export "f64x2.convert_low_i32x4_u") (param v128) (result v128)
    (f64x2.convert_low_i32x4_u (local.get 0)))
  (func (export "f64x2.promote_low_f32x4") (param v128) (result v128)
    (f64x2.promote_low_f32x4 (local.get 0)))
  (func (export "f32x4.demote_f64x2_zero") (param v128) (result v128)
    (f32x4.demote_f64x2_zero (local.get 0)))
)

;; i16x8.extadd_pairwise_i8x16_s/u
(assert_return
  (invoke "i16x8.extadd_pairwise_i8x16_s"
          (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16))
  (v128.const i16x8 3 7 11 15 19 23 27 31))
(assert_return
  (invoke "i16x8.extadd_pairwise_i8x16_u"
          (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16))
  (v128.const i16x8 3 7 11 15 19 23 27 31))

;; i32x4.extadd_pairwise_i16x8_s/u
(assert_return
  (invoke "i32x4.extadd_pairwise_i16x8_s"
          (v128.const i16x8 1 2 3 4 5 6 7 8))
  (v128.const i32x4 3 7 11 15))
(assert_return
  (invoke "i32x4.extadd_pairwise_i16x8_u"
          (v128.const i16x8 1 2 3 4 5 6 7 8))
  (v128.const i32x4 3 7 11 15))

  ;; i16x8.extmul_low/high_i8x16_s
  (assert_return
    (invoke "i16x8.extmul_low_i8x16_s"
            (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
            (v128.const i8x16 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115))
    (v128.const i16x8 100 202 306 412 520 630 742 856))
  (assert_return
    (invoke "i16x8.extmul_high_i8x16_s"
            (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
            (v128.const i8x16 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115))
    (v128.const i16x8 972 1090 1210 1332 1456 1582 1710 1840))

  ;; i16x8.extmul_low/high_i8x16_u
  (assert_return
    (invoke "i16x8.extmul_low_i8x16_u"
            (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
            (v128.const i8x16 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115))
    (v128.const i16x8 100 202 306 412 520 630 742 856))
  (assert_return
    (invoke "i16x8.extmul_high_i8x16_u"
            (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
            (v128.const i8x16 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115))
    (v128.const i16x8 972 1090 1210 1332 1456 1582 1710 1840))

;; i32x4.extmul_low/high_i16x8_s/u
(assert_return
  (invoke "i32x4.extmul_low_i16x8_s"
          (v128.const i16x8 1 2 3 4 5 6 7 8)
          (v128.const i16x8 1000 1001 1002 1003 1004 1005 1006 1007))
  (v128.const i32x4 1000 2002 3006 4012))
(assert_return
  (invoke "i32x4.extmul_low_i16x8_u"
          (v128.const i16x8 1 2 3 4 5 6 7 8)
          (v128.const i16x8 1000 1001 1002 1003 1004 1005 1006 1007))
  (v128.const i32x4 1000 2002 3006 4012))
(assert_return
  (invoke "i32x4.extmul_high_i16x8_s"
          (v128.const i16x8 1 2 3 4 5 6 7 8)
          (v128.const i16x8 1000 1001 1002 1003 1004 1005 1006 1007))
  (v128.const i32x4 5020 6030 7042 8056))
(assert_return
  (invoke "i32x4.extmul_high_i16x8_u"
          (v128.const i16x8 1 2 3 4 5 6 7 8)
          (v128.const i16x8 1000 1001 1002 1003 1004 1005 1006 1007))
  (v128.const i32x4 5020 6030 7042 8056))

;; i64x2.extmul_low/high_i32x4_s/u
(assert_return
  (invoke "i64x2.extmul_low_i32x4_s"
          (v128.const i32x4 1 2 3 4)
          (v128.const i32x4 1000 1001 1002 1003))
  (v128.const i64x2 1000 2002))
(assert_return
  (invoke "i64x2.extmul_low_i32x4_u"
          (v128.const i32x4 1 2 3 4)
          (v128.const i32x4 1000 1001 1002 1003))
  (v128.const i64x2 1000 2002))
(assert_return
  (invoke "i64x2.extmul_high_i32x4_s"
          (v128.const i32x4 1 2 3 4)
          (v128.const i32x4 1000 1001 1002 1003))
  (v128.const i64x2 3006 4012))
(assert_return
  (invoke "i64x2.extmul_high_i32x4_u"
          (v128.const i32x4 1 2 3 4)
          (v128.const i32x4 1000 1001 1002 1003))
  (v128.const i64x2 3006 4012))

;; i32x4.dot_i16x8_s
(assert_return
  (invoke "i32x4.dot_i16x8_s"
          (v128.const i16x8 1 2 3 4 5 6 7 8)
          (v128.const i16x8 1000 1001 1002 1003 1004 1005 1006 1007))
  (v128.const i32x4 3002 7018 11050 15098))

;; i16x8.extend_low/high_i8x16
(assert_return
  (invoke "i16x8.extend_low_i8x16_s"
          (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16))
  (v128.const i16x8 1 2 3 4 5 6 7 8))
(assert_return
  (invoke "i16x8.extend_low_i8x16_u"
          (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16))
  (v128.const i16x8 1 2 3 4 5 6 7 8))
(assert_return
  (invoke "i16x8.extend_high_i8x16_u"
          (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16))
  (v128.const i16x8 9 10 11 12 13 14 15 16))

;; i32x4.extend_low/high_i16x8
(assert_return
  (invoke "i32x4.extend_low_i16x8_s"
          (v128.const i16x8 1 2 3 4 5 6 7 8))
  (v128.const i32x4 1 2 3 4))
(assert_return
  (invoke "i32x4.extend_high_i16x8_s"
          (v128.const i16x8 1 2 3 4 5 6 7 8))
  (v128.const i32x4 5 6 7 8))
(assert_return
  (invoke "i32x4.extend_low_i16x8_u"
          (v128.const i16x8 1 2 3 4 5 6 7 8))
  (v128.const i32x4 1 2 3 4))
(assert_return
  (invoke "i32x4.extend_high_i16x8_u"
          (v128.const i16x8 1 2 3 4 5 6 7 8))
  (v128.const i32x4 5 6 7 8))

;; i64x2.extend_low/high_i32x4
(assert_return
  (invoke "i64x2.extend_low_i32x4_s"
          (v128.const i32x4 1 2 3 4))
  (v128.const i64x2 1 2))
(assert_return
  (invoke "i64x2.extend_low_i32x4_u"
          (v128.const i32x4 1 2 3 4))
  (v128.const i64x2 1 2))
(assert_return
  (invoke "i64x2.extend_high_i32x4_s"
          (v128.const i32x4 1 2 3 4))
  (v128.const i64x2 3 4))
(assert_return
  (invoke "i64x2.extend_high_i32x4_u"
          (v128.const i32x4 1 2 3 4))
  (v128.const i64x2 3 4))

;; i8x16.narrow_i16x8_s/u
(assert_return
  (invoke "i8x16.narrow_i16x8_s"
          (v128.const i16x8 1 2 3 4 5 6 7 8)
          (v128.const i16x8 1000 1001 1002 1003 1004 1005 1006 1007))
  (v128.const i8x16 1 2 3 4 5 6 7 8 127 127 127 127 127 127 127 127))
(assert_return
  (invoke "i8x16.narrow_i16x8_u"
          (v128.const i16x8 1 2 3 4 5 6 7 8)
          (v128.const i16x8 1000 1001 1002 1003 1004 1005 1006 1007))
  (v128.const i8x16 1 2 3 4 5 6 7 8 255 255 255 255 255 255 255 255))

;; i16x8.narrow_i32x4_s/u
(assert_return
  (invoke "i16x8.narrow_i32x4_s"
          (v128.const i32x4 1 2 3 4)
          (v128.const i32x4 1000 1001 1002 1003))
  (v128.const i16x8 1 2 3 4 1000 1001 1002 1003))
(assert_return
  (invoke "i16x8.narrow_i32x4_u"
          (v128.const i32x4 1 2 3 4)
          (v128.const i32x4 1000 1001 1002 1003))
  (v128.const i16x8 1 2 3 4 1000 1001 1002 1003))

;; i32x4.trunc_sat_f64x2_s_zero
(assert_return
  (invoke "i32x4.trunc_sat_f64x2_s_zero"
          (v128.const f64x2 1.0 2.0))
  (v128.const i32x4 1 2 0 0))

;; f64x2.convert_low_i32x4_s/u
(assert_return
  (invoke "f64x2.convert_low_i32x4_s"
          (v128.const i32x4 1 2 3 4))
  (v128.const f64x2 1.0 2.0))
(assert_return
  (invoke "f64x2.convert_low_i32x4_u"
          (v128.const i32x4 1 2 3 4))
  (v128.const f64x2 1.0 2.0))

;; f64x2.promote_low_f32x4
(assert_return
  (invoke "f64x2.promote_low_f32x4"
          (v128.const f32x4 1.0 2.0 3.0 4.0))
  (v128.const f64x2 1.0 2.0))

;; f32x4.demote_f64x2_zero
(assert_return
  (invoke "f32x4.demote_f64x2_zero"
          (v128.const f64x2 1.0 2.0))
  (v128.const f32x4 1.0 2.0 0.0 0.0))
