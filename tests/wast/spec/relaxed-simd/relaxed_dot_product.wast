;; Tests for relaxed dot products.

(module
    (func (export "i16x8.relaxed_dot_i8x16_i7x16_s") (param v128 v128) (result v128) (i16x8.relaxed_dot_i8x16_i7x16_s (local.get 0) (local.get 1)))
    (func (export "i32x4.relaxed_dot_i8x16_i7x16_add_s") (param v128 v128 v128) (result v128) (i32x4.relaxed_dot_i8x16_i7x16_add_s (local.get 0) (local.get 1) (local.get 2)))

    (func (export "i16x8.relaxed_dot_i8x16_i7x16_s_cmp") (param v128 v128) (result v128)
          (i16x8.eq
            (i16x8.relaxed_dot_i8x16_i7x16_s (local.get 0) (local.get 1))
            (i16x8.relaxed_dot_i8x16_i7x16_s (local.get 0) (local.get 1))))
    (func (export "i32x4.relaxed_dot_i8x16_i7x16_add_s_cmp") (param v128 v128 v128) (result v128)
          (i16x8.eq
            (i32x4.relaxed_dot_i8x16_i7x16_add_s (local.get 0) (local.get 1) (local.get 2))
            (i32x4.relaxed_dot_i8x16_i7x16_add_s (local.get 0) (local.get 1) (local.get 2))))
)

;; Simple values to ensure things are functional.
(assert_return (invoke "i16x8.relaxed_dot_i8x16_i7x16_s"
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15))
               (v128.const i16x8 1 13 41 85 145 221 313 421))

;; Test max and min i8 values;
(assert_return (invoke "i16x8.relaxed_dot_i8x16_i7x16_s"
                       (v128.const i8x16 -128 -128 127 127 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 127 127 127 127 0 0 0 0 0 0 0 0 0 0 0 0))
               (v128.const i16x8 -32512 32258 0 0 0 0 0 0))

;; signed * unsigned   : -128 *  129 * 2 = -33,024 saturated to -32,768
;; signed * signed     : -128 * -127 * 2 =  32,512
;; unsigned * unsigned :  128 *  129 * 2 =  33,024
(assert_return (invoke "i16x8.relaxed_dot_i8x16_i7x16_s"
                       (v128.const i8x16 -128 -128 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 -127 -127 0 0 0 0 0 0 0 0 0 0 0 0 0 0))
               (either
                 (v128.const i16x8 -32768 0 0 0 0 0 0 0)
                 (v128.const i16x8  32512 0 0 0 0 0 0 0)
                 (v128.const i16x8  33024 0 0 0 0 0 0 0)))

;; Simple values to ensure things are functional.
(assert_return (invoke "i32x4.relaxed_dot_i8x16_i7x16_add_s"
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i32x4 0 1 2 3))
               ;; intermediate result is [14, 126, 366, 734]
               (v128.const i32x4 14 127 368 737))

;; Test max and min i8 values;
(assert_return (invoke "i32x4.relaxed_dot_i8x16_i7x16_add_s"
                       (v128.const i8x16 -128 -128 -128 -128 127 127 127 127 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 127 127 127 127 127 127 127 127 0 0 0 0 0 0 0 0)
                       (v128.const i32x4 1 2 3 4))
               ;; intermediate result is [-65024, 64516, 0, 0]
               (v128.const i32x4 -65023 64518 3 4))

;; signed * unsigned   : -128 *  129 * 4 = -66,048 (+ 1) VPDPBUSD AVX2-VNNI or AVX512-VNNI
;; signed * unsigned with intermediate saturation :
;;   (-128 * 129) + (-128 * 129) = -33024 saturated to -32768 (PMADDUBSW)
;;   -32768 + -32768 = -65536 (+ 1)
;; signed * signed     : -128 * -127 * 4 =  65,024 (+ 1)
;; unsigned * unsigned :  128 *  129 * 2 =  66,048 (+ 1)
(assert_return (invoke "i32x4.relaxed_dot_i8x16_i7x16_add_s"
                       (v128.const i8x16 -128 -128 -128 -128 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 -127 -127 -127 -127 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i32x4 1 2 3 4))
               (either
                 (v128.const i32x4 -66047 2 3 4)
                 (v128.const i32x4 -65535 2 3 4)
                 (v128.const i32x4  65025 2 3 4)
                 (v128.const i32x4  66049 2 3 4)))

;; Check that multiple calls to the relaxed instruction with same inputs returns same results.

;; Test max and min i8 values;
(assert_return (invoke "i16x8.relaxed_dot_i8x16_i7x16_s_cmp"
                       (v128.const i8x16 -128 -128 127 127 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 127 127 127 127 0 0 0 0 0 0 0 0 0 0 0 0))
               (v128.const i16x8 -1 -1 -1 -1 -1 -1 -1 -1))

;; Test max and min i8 values;
(assert_return (invoke "i32x4.relaxed_dot_i8x16_i7x16_add_s_cmp"
                       (v128.const i8x16 -128 -128 -128 -128 127 127 127 127 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 127 127 127 127 127 127 127 127 0 0 0 0 0 0 0 0)
                       (v128.const i32x4 1 2 3 4))
               ;; intermediate result is [-65024, 64516, 0, 0]
               (v128.const i32x4 -1 -1 -1 -1))

;; signed * unsigned   : -128 *  129 * 2 = -33,024 saturated to -32,768
;; signed * signed     : -128 * -127 * 2 =  32,512
;; unsigned * unsigned :  128 *  129 * 2 =  33,024
(assert_return (invoke "i16x8.relaxed_dot_i8x16_i7x16_s_cmp"
                       (v128.const i8x16 -128 -128 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 -127 -127 0 0 0 0 0 0 0 0 0 0 0 0 0 0))
               (v128.const i16x8 -1 -1 -1 -1 -1 -1 -1 -1))

;; signed * unsigned   : -128 *  129 * 4 = -66,048 (+ 1) VPDPBUSD AVX2-VNNI or AVX512-VNNI
;; signed * unsigned with intermediate saturation :
;;   (-128 * 129) + (-128 * 129) = -33024 saturated to -32768 (PMADDUBSW)
;;   -32768 + -32768 = -65536 (+ 1)
;; signed * signed     : -128 * -127 * 4 =  65,024 (+ 1)
;; unsigned * unsigned :  128 *  129 * 2 =  66,048 (+ 1)
(assert_return (invoke "i32x4.relaxed_dot_i8x16_i7x16_add_s_cmp"
                       (v128.const i8x16 -128 -128 -128 -128 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 -127 -127 -127 -127 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i32x4 1 2 3 4))
               (v128.const i32x4 -1 -1 -1 -1))
