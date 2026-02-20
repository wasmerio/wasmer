;; Tests for f32x4.relaxed_madd, f32x4.relaxed_nmadd, f64x2.relaxed_madd, and f64x2.relaxed_nmadd.
;; `either` comes from https://github.com/WebAssembly/threads.

(module
    (func (export "f32x4.relaxed_madd") (param v128 v128 v128) (result v128) (f32x4.relaxed_madd (local.get 0) (local.get 1) (local.get 2)))
    (func (export "f32x4.relaxed_nmadd") (param v128 v128 v128) (result v128) (f32x4.relaxed_nmadd (local.get 0) (local.get 1) (local.get 2)))
    (func (export "f64x2.relaxed_nmadd") (param v128 v128 v128) (result v128) (f64x2.relaxed_nmadd (local.get 0) (local.get 1) (local.get 2)))
    (func (export "f64x2.relaxed_madd") (param v128 v128 v128) (result v128) (f64x2.relaxed_madd (local.get 0) (local.get 1) (local.get 2)))

    (func (export "f32x4.relaxed_madd_cmp") (param v128 v128 v128) (result v128)
          (f32x4.eq
            (f32x4.relaxed_madd (local.get 0) (local.get 1) (local.get 2))
            (f32x4.relaxed_madd (local.get 0) (local.get 1) (local.get 2))))
    (func (export "f32x4.relaxed_nmadd_cmp") (param v128 v128 v128) (result v128)
          (f32x4.eq
            (f32x4.relaxed_nmadd (local.get 0) (local.get 1) (local.get 2))
            (f32x4.relaxed_nmadd (local.get 0) (local.get 1) (local.get 2))))
    (func (export "f64x2.relaxed_nmadd_cmp") (param v128 v128 v128) (result v128)
          (f64x2.eq
            (f64x2.relaxed_nmadd (local.get 0) (local.get 1) (local.get 2))
            (f64x2.relaxed_nmadd (local.get 0) (local.get 1) (local.get 2))))
    (func (export "f64x2.relaxed_madd_cmp") (param v128 v128 v128) (result v128)
          (f64x2.eq
            (f64x2.relaxed_madd (local.get 0) (local.get 1) (local.get 2))
            (f64x2.relaxed_madd (local.get 0) (local.get 1) (local.get 2))))
)


;; FLT_MAX == 0x1.fffffep+127
;; FLT_MAX * 2 - FLT_MAX ==
;;   FLT_MAX (if fma)
;;   0       (if no fma)
;; from https://www.vinc17.net/software/fma-tests.c
(assert_return (invoke "f32x4.relaxed_madd"
                       (v128.const f32x4 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 )
                       (v128.const f32x4 2.0 2.0 2.0 2.0)
                       (v128.const f32x4 -0x1.fffffep+127 -0x1.fffffep+127 -0x1.fffffep+127 -0x1.fffffep+127))
               (either (v128.const f32x4 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127)
                       (v128.const f32x4 inf inf inf inf)))

;; Special values for float:
;; x            = 0x1.000004p+0 (1 + 2^-22)
;; y            = 0x1.0002p+0   (1 + 2^-15)
;; z            = -(1.0 + 0x0.0002p+0 + 0x0.000004p+0)
;;              = -0x1.000204p+0
;; x.y          = 1.0 + 0x0.0002p+0 + 0x0.000004p+0 + 0x1p-37 (round bit)
;; x.y+z        = 0 (2 roundings)
;; fma(x, y, z) = (0x1p-37) 2^-37
;; from https://accurate-algorithms.readthedocs.io/en/latest/ch09appendix.html#test-system-information
(assert_return (invoke "f32x4.relaxed_madd"
                       (v128.const f32x4 0x1.000004p+0 0x1.000004p+0 0x1.000004p+0 0x1.000004p+0)
                       (v128.const f32x4 0x1.0002p+0 0x1.0002p+0 0x1.0002p+0 0x1.0002p+0)
                       (v128.const f32x4 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0))
               (either (v128.const f32x4 0x1p-37 0x1p-37 0x1p-37 0x1p-37)
                       (v128.const f32x4 0 0 0 0)))
;; nmadd tests with negated x, same answers are expected.
(assert_return (invoke "f32x4.relaxed_nmadd"
                       (v128.const f32x4 -0x1.000004p+0 -0x1.000004p+0 -0x1.000004p+0 -0x1.000004p+0)
                       (v128.const f32x4 0x1.0002p+0 0x1.0002p+0 0x1.0002p+0 0x1.0002p+0)
                       (v128.const f32x4 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0))
               (either (v128.const f32x4 0x1p-37 0x1p-37 0x1p-37 0x1p-37)
                       (v128.const f32x4 0 0 0 0)))
;; nmadd tests with negated y, same answers are expected.
(assert_return (invoke "f32x4.relaxed_nmadd"
                       (v128.const f32x4 0x1.000004p+0 0x1.000004p+0 0x1.000004p+0 0x1.000004p+0)
                       (v128.const f32x4 -0x1.0002p+0 -0x1.0002p+0 -0x1.0002p+0 -0x1.0002p+0)
                       (v128.const f32x4 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0))
               (either (v128.const f32x4 0x1p-37 0x1p-37 0x1p-37 0x1p-37)
                       (v128.const f32x4 0 0 0 0)))

;; DBL_MAX = 0x1.fffffffffffffp+1023
;; DLB_MAX * 2 - DLB_MAX ==
;;   DLB_MAX (if fma)
;;   0       (if no fma)
;; form https://www.vinc17.net/software/fma-tests.c
;; from https://www.vinc17.net/software/fma-tests.c
(assert_return (invoke "f64x2.relaxed_madd"
                       (v128.const f64x2 0x1.fffffffffffffp+1023 0x1.fffffffffffffp+1023)
                       (v128.const f64x2 2.0 2.0)
                       (v128.const f64x2 -0x1.fffffffffffffp+1023 -0x1.fffffffffffffp+1023))
               (either (v128.const f64x2 0x1.fffffffffffffp+1023 0x1.fffffffffffffp+1023)
                       (v128.const f64x2 inf inf)))

;; Special values for double:
;; x            = 0x1.00000004p+0 (1 + 2^-30)
;; y            = 0x1.000002p+0   (1 + 2^-23)
;; z            = -(1.0 + 0x0.000002p+0 + 0x0.00000004p+0)
;;              = -0x1.00000204p+0
;; x.y          = 1.0 + 0x0.000002p+0 + 0x0.00000004p+0 + 0x1p-53 (round bit)
;; x.y+z        = 0 (2 roundings)
;; fma(x, y, z) = 0x1p-53
;; from https://accurate-algorithms.readthedocs.io/en/latest/ch09appendix.html#test-system-information
(assert_return (invoke "f64x2.relaxed_madd"
                       (v128.const f64x2 0x1.00000004p+0 0x1.00000004p+0)
                       (v128.const f64x2 0x1.000002p+0 0x1.000002p+0)
                       (v128.const f64x2 -0x1.00000204p+0 -0x1.00000204p+0))
               (either (v128.const f64x2 0x1p-53 0x1p-53)
                       (v128.const f64x2 0 0)))
;; nmadd tests with negated x, same answers are expected.
(assert_return (invoke "f64x2.relaxed_nmadd"
                       (v128.const f64x2 -0x1.00000004p+0 -0x1.00000004p+0)
                       (v128.const f64x2 0x1.000002p+0 0x1.000002p+0)
                       (v128.const f64x2 -0x1.00000204p+0 -0x1.00000204p+0))
               (either (v128.const f64x2 0x1p-53 0x1p-53)
                       (v128.const f64x2 0 0)))
;; nmadd tests with negated y, same answers are expected.
(assert_return (invoke "f64x2.relaxed_nmadd"
                       (v128.const f64x2 0x1.00000004p+0 0x1.00000004p+0)
                       (v128.const f64x2 -0x1.000002p+0 -0x1.000002p+0)
                       (v128.const f64x2 -0x1.00000204p+0 -0x1.00000204p+0))
               (either (v128.const f64x2 0x1p-53 0x1p-53)
                       (v128.const f64x2 0 0)))

;; Check that multiple calls to the relaxed instruction with same inputs returns same results.

;; FLT_MAX == 0x1.fffffep+127
;; FLT_MAX * 2 - FLT_MAX ==
;;   FLT_MAX (if fma)
;;   0       (if no fma)
;; from https://www.vinc17.net/software/fma-tests.c
(assert_return (invoke "f32x4.relaxed_madd_cmp"
                       (v128.const f32x4 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 )
                       (v128.const f32x4 2.0 2.0 2.0 2.0)
                       (v128.const f32x4 -0x1.fffffep+127 -0x1.fffffep+127 -0x1.fffffep+127 -0x1.fffffep+127))
               (v128.const i32x4 -1 -1 -1 -1))

;; Special values for float:
;; x            = 0x1.000004p+0 (1 + 2^-22)
;; y            = 0x1.0002p+0   (1 + 2^-15)
;; z            = -(1.0 + 0x0.0002p+0 + 0x0.000004p+0)
;;              = -0x1.000204p+0
;; x.y          = 1.0 + 0x0.0002p+0 + 0x0.000004p+0 + 0x1p-37 (round bit)
;; x.y+z        = 0 (2 roundings)
;; fma(x, y, z) = (0x1p-37) 2^-37
;; from https://accurate-algorithms.readthedocs.io/en/latest/ch09appendix.html#test-system-information
(assert_return (invoke "f32x4.relaxed_madd_cmp"
                       (v128.const f32x4 0x1.000004p+0 0x1.000004p+0 0x1.000004p+0 0x1.000004p+0)
                       (v128.const f32x4 0x1.0002p+0 0x1.0002p+0 0x1.0002p+0 0x1.0002p+0)
                       (v128.const f32x4 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0))
               (v128.const i32x4 -1 -1 -1 -1))
;; nmadd tests with negated x, same answers are expected.
(assert_return (invoke "f32x4.relaxed_nmadd_cmp"
                       (v128.const f32x4 -0x1.000004p+0 -0x1.000004p+0 -0x1.000004p+0 -0x1.000004p+0)
                       (v128.const f32x4 0x1.0002p+0 0x1.0002p+0 0x1.0002p+0 0x1.0002p+0)
                       (v128.const f32x4 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0))
               (v128.const i32x4 -1 -1 -1 -1))
;; nmadd tests with negated y, same answers are expected.
(assert_return (invoke "f32x4.relaxed_nmadd_cmp"
                       (v128.const f32x4 0x1.000004p+0 0x1.000004p+0 0x1.000004p+0 0x1.000004p+0)
                       (v128.const f32x4 -0x1.0002p+0 -0x1.0002p+0 -0x1.0002p+0 -0x1.0002p+0)
                       (v128.const f32x4 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0 -0x1.000204p+0))
               (v128.const i32x4 -1 -1 -1 -1))

;; DBL_MAX = 0x1.fffffffffffffp+1023
;; DLB_MAX * 2 - DLB_MAX ==
;;   DLB_MAX (if fma)
;;   0       (if no fma)
;; form https://www.vinc17.net/software/fma-tests.c
;; from https://www.vinc17.net/software/fma-tests.c
(assert_return (invoke "f64x2.relaxed_madd_cmp"
                       (v128.const f64x2 0x1.fffffffffffffp+1023 0x1.fffffffffffffp+1023)
                       (v128.const f64x2 2.0 2.0)
                       (v128.const f64x2 -0x1.fffffffffffffp+1023 -0x1.fffffffffffffp+1023))
               (v128.const i64x2 -1 -1))

;; Special values for double:
;; x            = 0x1.00000004p+0 (1 + 2^-30)
;; y            = 0x1.000002p+0   (1 + 2^-23)
;; z            = -(1.0 + 0x0.000002p+0 + 0x0.00000004p+0)
;;              = -0x1.00000204p+0
;; x.y          = 1.0 + 0x0.000002p+0 + 0x0.00000004p+0 + 0x1p-53 (round bit)
;; x.y+z        = 0 (2 roundings)
;; fma(x, y, z) = 0x1p-53
;; from https://accurate-algorithms.readthedocs.io/en/latest/ch09appendix.html#test-system-information
(assert_return (invoke "f64x2.relaxed_madd_cmp"
                       (v128.const f64x2 0x1.00000004p+0 0x1.00000004p+0)
                       (v128.const f64x2 0x1.000002p+0 0x1.000002p+0)
                       (v128.const f64x2 -0x1.00000204p+0 -0x1.00000204p+0))
               (v128.const i64x2 -1 -1))
;; nmadd tests with negated x, same answers are expected.
(assert_return (invoke "f64x2.relaxed_nmadd_cmp"
                       (v128.const f64x2 -0x1.00000004p+0 -0x1.00000004p+0)
                       (v128.const f64x2 0x1.000002p+0 0x1.000002p+0)
                       (v128.const f64x2 -0x1.00000204p+0 -0x1.00000204p+0))
               (v128.const i64x2 -1 -1))
;; nmadd tests with negated y, same answers are expected.
(assert_return (invoke "f64x2.relaxed_nmadd_cmp"
                       (v128.const f64x2 0x1.00000004p+0 0x1.00000004p+0)
                       (v128.const f64x2 -0x1.000002p+0 -0x1.000002p+0)
                       (v128.const f64x2 -0x1.00000204p+0 -0x1.00000204p+0))
               (v128.const i64x2 -1 -1))

;; Test that the non-deterministic choice of fusing and then rounding or
;; rounding multiple times in `relaxed_madd` is consistent throughout a
;; program's execution.
;;
;; This property is impossible to test exhaustively, so this is just a simple
;; smoke test for when the operands to a `relaxed_madd` are known statically
;; versus when they are dynamically supplied. This should, at least, catch
;; illegal constant-folding and -propagation by the compiler that leads to
;; inconsistent rounding behavior at compile time versus at run time.
;;
;; FLT_MAX == 0x1.fffffep+127
;; FLT_MAX * 2 - FLT_MAX ==
;;   FLT_MAX (if fma)
;;   0       (if no fma)
;; from https://www.vinc17.net/software/fma-tests.c
(module
  (func (export "test-consistent-nondeterminism") (param v128 v128 v128) (result v128)
    (f32x4.eq
      (f32x4.relaxed_madd (v128.const f32x4 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 )
                          (v128.const f32x4 2.0 2.0 2.0 2.0)
                          (v128.const f32x4 -0x1.fffffep+127 -0x1.fffffep+127 -0x1.fffffep+127 -0x1.fffffep+127))
      (f32x4.relaxed_madd (local.get 0)
                          (local.get 1)
                          (local.get 2))
    )
  )
)
(assert_return (invoke "test-consistent-nondeterminism"
                       (v128.const f32x4 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 0x1.fffffep+127 )
                       (v128.const f32x4 2.0 2.0 2.0 2.0)
                       (v128.const f32x4 -0x1.fffffep+127 -0x1.fffffep+127 -0x1.fffffep+127 -0x1.fffffep+127))
               (v128.const i32x4 -1 -1 -1 -1))
