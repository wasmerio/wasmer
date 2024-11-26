;; Tests for i32x4 arithmetic operations on major boundary values and all special values.


(module
  (func (export "i32x4.extadd_pairwise_i16x8_s") (param v128) (result v128) (i32x4.extadd_pairwise_i16x8_s (local.get 0)))
  (func (export "i32x4.extadd_pairwise_i16x8_u") (param v128) (result v128) (i32x4.extadd_pairwise_i16x8_u (local.get 0)))
)


;; i32x4.extadd_pairwise_i16x8_s
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_s" (v128.const i16x8 0 0 0 0 0 0 0 0))
                                                       (v128.const i32x4 0 0 0 0))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_s" (v128.const i16x8 1 1 1 1 1 1 1 1))
                                                       (v128.const i32x4 2 2 2 2))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_s" (v128.const i16x8 -1 -1 -1 -1 -1 -1 -1 -1))
                                                       (v128.const i32x4 -2 -2 -2 -2))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_s" (v128.const i16x8 32766 32766 32766 32766 32766 32766 32766 32766))
                                                       (v128.const i32x4 65532 65532 65532 65532))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_s" (v128.const i16x8 -32767 -32767 -32767 -32767 -32767 -32767 -32767 -32767))
                                                       (v128.const i32x4 -65534 -65534 -65534 -65534))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_s" (v128.const i16x8 -32768 -32768 -32768 -32768 -32768 -32768 -32768 -32768))
                                                       (v128.const i32x4 -65536 -65536 -65536 -65536))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_s" (v128.const i16x8 32767 32767 32767 32767 32767 32767 32767 32767))
                                                       (v128.const i32x4 65534 65534 65534 65534))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_s" (v128.const i16x8 65535 65535 65535 65535 65535 65535 65535 65535))
                                                       (v128.const i32x4 -2 -2 -2 -2))

;; i32x4.extadd_pairwise_i16x8_u
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_u" (v128.const i16x8 0 0 0 0 0 0 0 0))
                                                       (v128.const i32x4 0 0 0 0))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_u" (v128.const i16x8 1 1 1 1 1 1 1 1))
                                                       (v128.const i32x4 2 2 2 2))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_u" (v128.const i16x8 -1 -1 -1 -1 -1 -1 -1 -1))
                                                       (v128.const i32x4 131070 131070 131070 131070))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_u" (v128.const i16x8 32766 32766 32766 32766 32766 32766 32766 32766))
                                                       (v128.const i32x4 65532 65532 65532 65532))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_u" (v128.const i16x8 -32767 -32767 -32767 -32767 -32767 -32767 -32767 -32767))
                                                       (v128.const i32x4 65538 65538 65538 65538))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_u" (v128.const i16x8 -32768 -32768 -32768 -32768 -32768 -32768 -32768 -32768))
                                                       (v128.const i32x4 65536 65536 65536 65536))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_u" (v128.const i16x8 32767 32767 32767 32767 32767 32767 32767 32767))
                                                       (v128.const i32x4 65534 65534 65534 65534))
(assert_return (invoke "i32x4.extadd_pairwise_i16x8_u" (v128.const i16x8 65535 65535 65535 65535 65535 65535 65535 65535))
                                                       (v128.const i32x4 131070 131070 131070 131070))

;; type check
(assert_invalid (module (func (result v128) (i32x4.extadd_pairwise_i16x8_s (i32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) (i32x4.extadd_pairwise_i16x8_u (i32.const 0)))) "type mismatch")

;; Test operation with empty argument

(assert_invalid
  (module
    (func $i32x4.extadd_pairwise_i16x8_s-arg-empty (result v128)
      (i32x4.extadd_pairwise_i16x8_s)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $i32x4.extadd_pairwise_i16x8_u-arg-empty (result v128)
      (i32x4.extadd_pairwise_i16x8_u)
    )
  )
  "type mismatch"
)

