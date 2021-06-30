;; Test that constant folding an operation that overflows doesn't produce an
;; undefined value. Changing these tests to move the constants to the
;; assert_return line hides the bug.

(module
  ;; shl i32
  (func (export "shl1_i32") (result i32)
    i32.const 235
    i32.const 0
    i32.shl
  )
  (func (export "shl2_i32") (result i32)
    i32.const 235
    i32.const 32
    i32.shl
  )
  (func (export "shl3_i32") (result i32)
    i32.const 235
    i32.const 100
    i32.shl
  )
  (func (export "shl4_i32") (result i32)
    i32.const 235
    i32.const -32
    i32.shl
  )
  (func (export "shl5_i32") (result i32)
    i32.const 235
    i32.const -100
    i32.shl
  )
  ;; shl i64
  (func (export "shl1_i64") (result i64)
    i64.const 235
    i64.const 0
    i64.shl
  )
  (func (export "shl2_i64") (result i64)
    i64.const 235
    i64.const 64
    i64.shl
  )
  (func (export "shl3_i64") (result i64)
    i64.const 235
    i64.const 100
    i64.shl
  )
  (func (export "shl4_i64") (result i64)
    i64.const 235
    i64.const -64
    i64.shl
  )
  (func (export "shl5_i64") (result i64)
    i64.const 235
    i64.const -100
    i64.shl
  )

  ;; shr_u i32
  (func (export "shr_u1_i32") (result i32)
    i32.const 235
    i32.const 0
    i32.shr_u
  )
  (func (export "shr_u2_i32") (result i32)
    i32.const 235
    i32.const 32
    i32.shr_u
  )
  (func (export "shr_u3_i32") (result i32)
    i32.const 235
    i32.const 100
    i32.shr_u
  )
  (func (export "shr_u4_i32") (result i32)
    i32.const 235
    i32.const -32
    i32.shr_u
  )
  (func (export "shr_u5_i32") (result i32)
    i32.const 235
    i32.const -100
    i32.shr_u
  )

  ;; shr_u i64
  (func (export "shr_u1_i64") (result i64)
    i64.const 235
    i64.const 0
    i64.shr_u
  )
  (func (export "shr_u2_i64") (result i64)
    i64.const 235
    i64.const 64
    i64.shr_u
  )
  (func (export "shr_u3_i64") (result i64)
    i64.const 235
    i64.const 100
    i64.shr_u
  )
  (func (export "shr_u4_i64") (result i64)
    i64.const 235
    i64.const -64
    i64.shr_u
  )
  (func (export "shr_u5_i64") (result i64)
    i64.const 235
    i64.const -100
    i64.shr_u
  )

  ;; shr_s i32
  (func (export "shr_s1_i32") (result i32)
    i32.const 235
    i32.const 0
    i32.shr_s
  )
  (func (export "shr_s2_i32") (result i32)
    i32.const 235
    i32.const 32
    i32.shr_s
  )
  (func (export "shr_s3_i32") (result i32)
    i32.const 235
    i32.const 100
    i32.shr_s
  )
  (func (export "shr_s4_i32") (result i32)
    i32.const 235
    i32.const -32
    i32.shr_s
  )
  (func (export "shr_s5_i32") (result i32)
    i32.const 235
    i32.const -100
    i32.shr_s
  )

  ;; shr_s i64
  (func (export "shr_s1_i64") (result i64)
    i64.const 235
    i64.const 0
    i64.shr_s
  )
  (func (export "shr_s2_i64") (result i64)
    i64.const 235
    i64.const 64
    i64.shr_s
  )
  (func (export "shr_s3_i64") (result i64)
    i64.const 235
    i64.const 100
    i64.shr_s
  )
  (func (export "shr_s4_i64") (result i64)
    i64.const 235
    i64.const -64
    i64.shr_s
  )
  (func (export "shr_s5_i64") (result i64)
    i64.const 235
    i64.const -100
    i64.shr_s
  )

  ;; rotl i32
  (func (export "rotl1_i32") (result i32)
    i32.const 235
    i32.const 0
    i32.rotl
  )
  (func (export "rotl2_i32") (result i32)
    i32.const 235
    i32.const 32
    i32.rotl
  )
  (func (export "rotl3_i32") (result i32)
    i32.const 235
    i32.const 100
    i32.rotl
  )
  (func (export "rotl4_i32") (result i32)
    i32.const 235
    i32.const -32
    i32.rotl
  )
  (func (export "rotl5_i32") (result i32)
    i32.const 235
    i32.const -100
    i32.rotl
  )

  ;; rotl i64
  (func (export "rotl1_i64") (result i64)
    i64.const 235
    i64.const 0
    i64.rotl
  )
  (func (export "rotl2_i64") (result i64)
    i64.const 235
    i64.const 64
    i64.rotl
  )
  (func (export "rotl3_i64") (result i64)
    i64.const 235
    i64.const 100
    i64.rotl
  )
  (func (export "rotl4_i64") (result i64)
    i64.const 235
    i64.const -64
    i64.rotl
  )
  (func (export "rotl5_i64") (result i64)
    i64.const 235
    i64.const -100
    i64.rotl
  )

  ;; rotr i32
  (func (export "rotr1_i32") (result i32)
    i32.const 235
    i32.const 0
    i32.rotr
  )
  (func (export "rotr2_i32") (result i32)
    i32.const 235
    i32.const 32
    i32.rotr
  )
  (func (export "rotr3_i32") (result i32)
    i32.const 235
    i32.const 100
    i32.rotr
  )
  (func (export "rotr4_i32") (result i32)
    i32.const 235
    i32.const -32
    i32.rotr
  )
  (func (export "rotr5_i32") (result i32)
    i32.const 235
    i32.const -100
    i32.rotr
  )

  ;; rotr i64
  (func (export "rotr1_i64") (result i64)
    i64.const 235
    i64.const 0
    i64.rotr
  )
  (func (export "rotr2_i64") (result i64)
    i64.const 235
    i64.const 64
    i64.rotr
  )
  (func (export "rotr3_i64") (result i64)
    i64.const 235
    i64.const 100
    i64.rotr
  )
  (func (export "rotr4_i64") (result i64)
    i64.const 235
    i64.const -64
    i64.rotr
  )
  (func (export "rotr5_i64") (result i64)
    i64.const 235
    i64.const -100
    i64.rotr
  )
)

(assert_return (invoke "shl1_i32") (i32.const 235))
(assert_return (invoke "shl2_i32") (i32.const 235))
(assert_return (invoke "shl3_i32") (i32.const 3760))
(assert_return (invoke "shl4_i32") (i32.const 235))
(assert_return (invoke "shl5_i32") (i32.const -1342177280))

(assert_return (invoke "shl1_i64") (i64.const 235))
(assert_return (invoke "shl2_i64") (i64.const 235))
(assert_return (invoke "shl3_i64") (i64.const 16149077032960))
(assert_return (invoke "shl4_i64") (i64.const 235))
(assert_return (invoke "shl5_i64") (i64.const 63082332160))

(assert_return (invoke "shr_u1_i32") (i32.const 235))
(assert_return (invoke "shr_u2_i32") (i32.const 235))
(assert_return (invoke "shr_u3_i32") (i32.const 14))
(assert_return (invoke "shr_u4_i32") (i32.const 235))
(assert_return (invoke "shr_u5_i32") (i32.const 0))

(assert_return (invoke "shr_u1_i64") (i64.const 235))
(assert_return (invoke "shr_u2_i64") (i64.const 235))
(assert_return (invoke "shr_u3_i64") (i64.const 0))
(assert_return (invoke "shr_u4_i64") (i64.const 235))
(assert_return (invoke "shr_u5_i64") (i64.const 0))

(assert_return (invoke "shr_s1_i32") (i32.const 235))
(assert_return (invoke "shr_s2_i32") (i32.const 235))
(assert_return (invoke "shr_s3_i32") (i32.const 14))
(assert_return (invoke "shr_s4_i32") (i32.const 235))
(assert_return (invoke "shr_s5_i32") (i32.const 0))

(assert_return (invoke "shr_s1_i64") (i64.const 235))
(assert_return (invoke "shr_s2_i64") (i64.const 235))
(assert_return (invoke "shr_s3_i64") (i64.const 0))
(assert_return (invoke "shr_s4_i64") (i64.const 235))
(assert_return (invoke "shr_s5_i64") (i64.const 0))

(assert_return (invoke "rotl1_i32") (i32.const 235))
(assert_return (invoke "rotl2_i32") (i32.const 235))
(assert_return (invoke "rotl3_i32") (i32.const 3760))
(assert_return (invoke "rotl4_i32") (i32.const 235))
(assert_return (invoke "rotl5_i32") (i32.const -1342177266))

(assert_return (invoke "rotl1_i64") (i64.const 235))
(assert_return (invoke "rotl2_i64") (i64.const 235))
(assert_return (invoke "rotl3_i64") (i64.const 16149077032960))
(assert_return (invoke "rotl4_i64") (i64.const 235))
(assert_return (invoke "rotl5_i64") (i64.const 63082332160))

(assert_return (invoke "rotr1_i32") (i32.const 235))
(assert_return (invoke "rotr2_i32") (i32.const 235))
(assert_return (invoke "rotr3_i32") (i32.const -1342177266))
(assert_return (invoke "rotr4_i32") (i32.const 235))
(assert_return (invoke "rotr5_i32") (i32.const 3760))

(assert_return (invoke "rotr1_i64") (i64.const 235))
(assert_return (invoke "rotr2_i64") (i64.const 235))
(assert_return (invoke "rotr3_i64") (i64.const 63082332160))
(assert_return (invoke "rotr4_i64") (i64.const 235))
(assert_return (invoke "rotr5_i64") (i64.const 16149077032960))
