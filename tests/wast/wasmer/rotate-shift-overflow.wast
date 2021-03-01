;; Test that constant folding which overflows doesn't produce an undefined value.
;; Changing these tests to move the constants to the caller hides the bug.

(module
  ;; shl
  (func (export "shl1") (result i32)
    i32.const 235
    i32.const 0
    i32.shl
  )
  (func (export "shl2") (result i32)
    i32.const 235
    i32.const 32
    i32.shl
  )
  (func (export "shl3") (result i32)
    i32.const 235
    i32.const 100
    i32.shl
  )
  (func (export "shl4") (result i32)
    i32.const 235
    i32.const -32
    i32.shl
  )
  (func (export "shl5") (result i32)
    i32.const 235
    i32.const -100
    i32.shl
  )

  ;; shr_u
  (func (export "shr_u1") (result i32)
    i32.const 235
    i32.const 0
    i32.shr_u
  )
  (func (export "shr_u2") (result i32)
    i32.const 235
    i32.const 32
    i32.shr_u
  )
  (func (export "shr_u3") (result i32)
    i32.const 235
    i32.const 100
    i32.shr_u
  )
  (func (export "shr_u4") (result i32)
    i32.const 235
    i32.const -32
    i32.shr_u
  )
  (func (export "shr_u5") (result i32)
    i32.const 235
    i32.const -100
    i32.shr_u
  )

  ;; shr_s
  (func (export "shr_s1") (result i32)
    i32.const 235
    i32.const 0
    i32.shr_s
  )
  (func (export "shr_s2") (result i32)
    i32.const 235
    i32.const 32
    i32.shr_s
  )
  (func (export "shr_s3") (result i32)
    i32.const 235
    i32.const 100
    i32.shr_s
  )
  (func (export "shr_s4") (result i32)
    i32.const 235
    i32.const -32
    i32.shr_s
  )
  (func (export "shr_s5") (result i32)
    i32.const 235
    i32.const -100
    i32.shr_s
  )

  ;; rotl
  (func (export "rotl1") (result i32)
    i32.const 235
    i32.const 0
    i32.rotl
  )
  (func (export "rotl2") (result i32)
    i32.const 235
    i32.const 32
    i32.rotl
  )
  (func (export "rotl3") (result i32)
    i32.const 235
    i32.const 100
    i32.rotl
  )
  (func (export "rotl4") (result i32)
    i32.const 235
    i32.const -32
    i32.rotl
  )
  (func (export "rotl5") (result i32)
    i32.const 235
    i32.const -100
    i32.rotl
  )

  ;; rotr
  (func (export "rotr1") (result i32)
    i32.const 235
    i32.const 0
    i32.rotr
  )
  (func (export "rotr2") (result i32)
    i32.const 235
    i32.const 32
    i32.rotr
  )
  (func (export "rotr3") (result i32)
    i32.const 235
    i32.const 100
    i32.rotr
  )
  (func (export "rotr4") (result i32)
    i32.const 235
    i32.const -32
    i32.rotr
  )
  (func (export "rotr5") (result i32)
    i32.const 235
    i32.const -100
    i32.rotr
  )
)

(assert_return (invoke "shl1") (i32.const 235))
(assert_return (invoke "shl2") (i32.const 235))
(assert_return (invoke "shl3") (i32.const 3760))
(assert_return (invoke "shl4") (i32.const 235))
(assert_return (invoke "shl5") (i32.const -1342177280))

(assert_return (invoke "shr_u1") (i32.const 235))
(assert_return (invoke "shr_u2") (i32.const 235))
(assert_return (invoke "shr_u3") (i32.const 14))
(assert_return (invoke "shr_u4") (i32.const 235))
(assert_return (invoke "shr_u5") (i32.const 0))

(assert_return (invoke "shr_s1") (i32.const 235))
(assert_return (invoke "shr_s2") (i32.const 235))
(assert_return (invoke "shr_s3") (i32.const 14))
(assert_return (invoke "shr_s4") (i32.const 235))
(assert_return (invoke "shr_s5") (i32.const 0))

(assert_return (invoke "rotl1") (i32.const 235))
(assert_return (invoke "rotl2") (i32.const 235))
(assert_return (invoke "rotl3") (i32.const 3760))
(assert_return (invoke "rotl4") (i32.const 235))
(assert_return (invoke "rotl5") (i32.const -1342177266))

(assert_return (invoke "rotr1") (i32.const 235))
(assert_return (invoke "rotr2") (i32.const 235))
(assert_return (invoke "rotr3") (i32.const -1342177266))
(assert_return (invoke "rotr4") (i32.const 235))
(assert_return (invoke "rotr5") (i32.const 3760))
