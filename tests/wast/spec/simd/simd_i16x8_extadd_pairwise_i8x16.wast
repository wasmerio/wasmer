;; Tests for i16x8 arithmetic operations on major boundary values and all special values.


(module
  (func (export "i16x8.extadd_pairwise_i8x16_s") (param v128) (result v128) (i16x8.extadd_pairwise_i8x16_s (local.get 0)))
  (func (export "i16x8.extadd_pairwise_i8x16_u") (param v128) (result v128) (i16x8.extadd_pairwise_i8x16_u (local.get 0)))
)


;; i16x8.extadd_pairwise_i8x16_s
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_s" (v128.const i8x16 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0))
                                                       (v128.const i16x8 0 0 0 0 0 0 0 0))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_s" (v128.const i8x16 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1))
                                                       (v128.const i16x8 2 2 2 2 2 2 2 2))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_s" (v128.const i8x16 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1))
                                                       (v128.const i16x8 -2 -2 -2 -2 -2 -2 -2 -2))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_s" (v128.const i8x16 126 126 126 126 126 126 126 126 126 126 126 126 126 126 126 126))
                                                       (v128.const i16x8 252 252 252 252 252 252 252 252))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_s" (v128.const i8x16 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127))
                                                       (v128.const i16x8 -254 -254 -254 -254 -254 -254 -254 -254))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_s" (v128.const i8x16 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128))
                                                       (v128.const i16x8 -256 -256 -256 -256 -256 -256 -256 -256))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_s" (v128.const i8x16 127 127 127 127 127 127 127 127 127 127 127 127 127 127 127 127))
                                                       (v128.const i16x8 254 254 254 254 254 254 254 254))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_s" (v128.const i8x16 255 255 255 255 255 255 255 255 255 255 255 255 255 255 255 255))
                                                       (v128.const i16x8 -2 -2 -2 -2 -2 -2 -2 -2))

;; i16x8.extadd_pairwise_i8x16_u
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_u" (v128.const i8x16 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0))
                                                       (v128.const i16x8 0 0 0 0 0 0 0 0))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_u" (v128.const i8x16 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1))
                                                       (v128.const i16x8 2 2 2 2 2 2 2 2))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_u" (v128.const i8x16 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1))
                                                       (v128.const i16x8 510 510 510 510 510 510 510 510))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_u" (v128.const i8x16 126 126 126 126 126 126 126 126 126 126 126 126 126 126 126 126))
                                                       (v128.const i16x8 252 252 252 252 252 252 252 252))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_u" (v128.const i8x16 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127 -127))
                                                       (v128.const i16x8 258 258 258 258 258 258 258 258))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_u" (v128.const i8x16 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128 -128))
                                                       (v128.const i16x8 256 256 256 256 256 256 256 256))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_u" (v128.const i8x16 127 127 127 127 127 127 127 127 127 127 127 127 127 127 127 127))
                                                       (v128.const i16x8 254 254 254 254 254 254 254 254))
(assert_return (invoke "i16x8.extadd_pairwise_i8x16_u" (v128.const i8x16 255 255 255 255 255 255 255 255 255 255 255 255 255 255 255 255))
                                                       (v128.const i16x8 510 510 510 510 510 510 510 510))

;; type check
(assert_invalid (module (func (result v128) (i16x8.extadd_pairwise_i8x16_s (i32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) (i16x8.extadd_pairwise_i8x16_u (i32.const 0)))) "type mismatch")

;; Test operation with empty argument

(assert_invalid
  (module
    (func $i16x8.extadd_pairwise_i8x16_s-arg-empty (result v128)
      (i16x8.extadd_pairwise_i8x16_s)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $i16x8.extadd_pairwise_i8x16_u-arg-empty (result v128)
      (i16x8.extadd_pairwise_i8x16_u)
    )
  )
  "type mismatch"
)

