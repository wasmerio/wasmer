;; Tests for i64x2 [abs] operations.

(module
  (func (export "i64x2.abs") (param v128) (result v128) (i64x2.abs (local.get 0)))
  (func (export "i64x2.abs_with_const_0") (result v128) (i64x2.abs (v128.const i64x2 -9223372036854775808 9223372036854775807)))
)

(assert_return (invoke "i64x2.abs" (v128.const i64x2 1 1))
                                   (v128.const i64x2 1 1))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -1 -1))
                                   (v128.const i64x2 1 1))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 18446744073709551615 18446744073709551615))
                                   (v128.const i64x2 1 1))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 0xffffffffffffffff 0xffffffffffffffff))
                                   (v128.const i64x2 0x1 0x1))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 9223372036854775808 9223372036854775808))
                                   (v128.const i64x2 9223372036854775808 9223372036854775808))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -9223372036854775808 -9223372036854775808))
                                   (v128.const i64x2 9223372036854775808 9223372036854775808))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -0x8000000000000000 -0x8000000000000000))
                                   (v128.const i64x2 0x8000000000000000 0x8000000000000000))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 0x8000000000000000 0x8000000000000000))
                                   (v128.const i64x2 0x8000000000000000 0x8000000000000000))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 01_2_3 01_2_3))
                                   (v128.const i64x2 01_2_3 01_2_3))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -01_2_3 -01_2_3))
                                   (v128.const i64x2 123 123))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 0x80 0x80))
                                   (v128.const i64x2 0x80 0x80))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -0x80 -0x80))
                                   (v128.const i64x2 0x80 0x80))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 0x0_8_0 0x0_8_0))
                                   (v128.const i64x2 0x0_8_0 0x0_8_0))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -0x0_8_0 -0x0_8_0))
                                   (v128.const i64x2 0x80 0x80))

;; Const vs const
(assert_return (invoke "i64x2.abs_with_const_0") (v128.const i64x2 9223372036854775808 9223372036854775807))

;; Param vs const

;; Test different lanes go through different if-then clauses
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -9223372036854775808 9223372036854775807))
                                   (v128.const i64x2 9223372036854775808 9223372036854775807))

;; Test opposite signs of zero
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -0 -0))
                                   (v128.const i64x2 -0 -0))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 +0 0))
                                   (v128.const i64x2 +0 0))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 -0 -0))
                                   (v128.const i64x2 -0 -0))
(assert_return (invoke "i64x2.abs" (v128.const i64x2 +0 +0))
                                   (v128.const i64x2 +0 +0))

;; Unknown operators

;; Type check
(assert_invalid (module (func (result v128) (i64x2.abs (f32.const 0.0)))) "type mismatch")

;; Test operation with empty argument

(assert_invalid
  (module
    (func $i64x2.abs-arg-empty (result v128)
      (i64x2.abs)
    )
  )
  "type mismatch"
)

;; Combination
(module
  (func (export "i64x2.abs-i64x2.abs") (param v128) (result v128) (i64x2.abs (i64x2.abs (local.get 0))))
)

(assert_return (invoke "i64x2.abs-i64x2.abs" (v128.const i64x2 -1 -1))
                                             (v128.const i64x2 1 1))
