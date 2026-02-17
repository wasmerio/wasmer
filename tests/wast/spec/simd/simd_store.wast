;; v128.store operator with normal argument (e.g. (i8x16, i16x8, i32x4, f32x4))

(module
  (memory 1)
  (func (export "v128.store_i8x16") (result v128)
    (v128.store (i32.const 0) (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15))
    (v128.load (i32.const 0))
  )
  (func (export "v128.store_i16x8") (result v128)
    (v128.store (i32.const 0) (v128.const i16x8 0 1 2 3 4 5 6 7))
    (v128.load (i32.const 0))
  )
  (func (export "v128.store_i16x8_2") (result v128)
    (v128.store (i32.const 0) (v128.const i16x8 012_345 012_345 012_345 012_345 012_345 012_345 012_345 012_345))
    (v128.load (i32.const 0))
  )
  (func (export "v128.store_i16x8_3") (result v128)
    (v128.store (i32.const 0) (v128.const i16x8 0x0_1234 0x0_1234 0x0_1234 0x0_1234 0x0_1234 0x0_1234 0x0_1234 0x0_1234))
    (v128.load (i32.const 0))
  )
  (func (export "v128.store_i32x4") (result v128)
    (v128.store (i32.const 0) (v128.const i32x4 0 1 2 3))
    (v128.load (i32.const 0))
  )
  (func (export "v128.store_i32x4_2") (result v128)
    (v128.store (i32.const 0) (v128.const i32x4 0_123_456_789 0_123_456_789 0_123_456_789 0_123_456_789))
    (v128.load (i32.const 0))
  )
  (func (export "v128.store_i32x4_3") (result v128)
    (v128.store (i32.const 0) (v128.const i32x4 0x0_1234_5678 0x0_1234_5678 0x0_1234_5678 0x0_1234_5678))
    (v128.load (i32.const 0))
  )

  (func (export "v128.store_f32x4") (result v128)
    (v128.store (i32.const 0) (v128.const f32x4 0 1 2 3))
    (v128.load (i32.const 0))
  )
)

(assert_return (invoke "v128.store_i8x16") (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15))
(assert_return (invoke "v128.store_i16x8") (v128.const i16x8 0 1 2 3 4 5 6 7))
(assert_return (invoke "v128.store_i16x8_2") (v128.const i16x8 12345 12345 12345 12345 12345 12345 12345 12345))
(assert_return (invoke "v128.store_i16x8_3") (v128.const i16x8 0x1234 0x1234 0x1234 0x1234 0x1234 0x1234 0x1234 0x1234))
(assert_return (invoke "v128.store_i32x4") (v128.const i32x4 0 1 2 3))
(assert_return (invoke "v128.store_i32x4_2") (v128.const i32x4 123456789 123456789 123456789 123456789))
(assert_return (invoke "v128.store_i32x4_3") (v128.const i32x4 0x12345678 0x12345678 0x12345678 0x12345678))
(assert_return (invoke "v128.store_f32x4") (v128.const f32x4 0 1 2 3))


;; v128.store operator as the argument of control constructs and instructions

(module
  (memory 1)
  (func (export "as-block-value")
    (block (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0)))
  )
  (func (export "as-loop-value")
    (loop (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0)))
  )
  (func (export "as-br-value")
    (block (br 0 (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0))))
  )
  (func (export "as-br_if-value")
    (block
      (br_if 0 (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0)) (i32.const 1))
    )
  )
  (func (export "as-br_if-value-cond")
    (block
      (br_if 0 (i32.const 6) (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0)))
    )
  )
  (func (export "as-br_table-value")
    (block
      (br_table 0 (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0)) (i32.const 1))
    )
  )
  (func (export "as-return-value")
    (return (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0)))
  )
  (func (export "as-if-then")
    (if (i32.const 1) (then (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0))))
  )
  (func (export "as-if-else")
    (if (i32.const 0) (then) (else (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0))))
  )
)

(assert_return (invoke "as-block-value"))
(assert_return (invoke "as-loop-value"))
(assert_return (invoke "as-br-value"))
(assert_return (invoke "as-br_if-value"))
(assert_return (invoke "as-br_if-value-cond"))
(assert_return (invoke "as-br_table-value"))
(assert_return (invoke "as-return-value"))
(assert_return (invoke "as-if-then"))
(assert_return (invoke "as-if-else"))


;; Unknown operator(e.g. v128.store8, v128.store16, v128.store32)

(assert_malformed
  (module quote
    "(memory 1)"
    "(func (v128.store8 (i32.const 0) (v128.const i32x4 0 0 0 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1)"
    "(func (v128.store16 (i32.const 0) (v128.const i32x4 0 0 0 0)))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1)"
    "(func (v128.store32 (i32.const 0) (v128.const i32x4 0 0 0 0)))"
  )
  "unknown operator"
)


;; Type mismatched (e.g. v128.load(f32.const 0), type address empty)

(assert_invalid
  (module (memory 1) (func (v128.store (f32.const 0) (v128.const i32x4 0 0 0 0))))
  "type mismatch"
)
(assert_invalid
  (module (memory 1) (func (local v128) (block (br_if 0 (v128.store)))))
  "type mismatch"
)
(assert_invalid
  (module (memory 1) (func (result v128) (v128.store (i32.const 0) (v128.const i32x4 0 0 0 0))))
  "type mismatch"
)


;; Test operation with empty argument

(assert_invalid
  (module (memory 0)
    (func $v128.store-1st-arg-empty
      (v128.store (v128.const i32x4 0 0 0 0))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module (memory 0)
    (func $v128.store-2nd-arg-empty
      (v128.store (i32.const 0))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module (memory 0)
    (func $v128.store-arg-empty
      (v128.store)
    )
  )
  "type mismatch"
)
