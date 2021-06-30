;; Load and Zero extend test cases

(module
  (memory 1)
  (data (i32.const 0) "\00\01\02\03\04\05\06\07\08\09\0A\0B\0C\0D\0E\0F\80\81\82\83\84\85\86\87\88\89")
  (data (i32.const 65520) "\0A\0B\0C\0D\0E\0F\80\81\82\83\84\85\86\87\88\89")

  (func (export "v128.load32_zero") (param $0 i32) (result v128)
    (v128.load32_zero (local.get $0))
  )
  (func (export "v128.load64_zero") (param $0 i32) (result v128)
    (v128.load64_zero (local.get $0))
  )

  ;; load by a constant amount
  (func (export "v128.load32_zero_const0") (result v128)
    (v128.load32_zero (i32.const 0))
  )
  (func (export "v128.load64_zero_const8") (result v128)
    (v128.load64_zero (i32.const 8))
  )

  ;; load data with different offset/align arguments
  ;; i16x8
  (func (export "v128.load32_zero_offset0") (param $0 i32) (result v128)
    (v128.load32_zero offset=0 (local.get $0))
  )
  (func (export "v128.load32_zero_align1") (param $0 i32) (result v128)
    (v128.load32_zero align=1 (local.get $0))
  )
  (func (export "v128.load32_zero_offset0_align1") (param $0 i32) (result v128)
    (v128.load32_zero offset=0 align=1 (local.get $0))
  )
  (func (export "v128.load32_zero_offset1_align1") (param $0 i32) (result v128)
    (v128.load32_zero offset=1 align=1 (local.get $0))
  )
  (func (export "v128.load32_zero_offset10_align4") (param $0 i32) (result v128)
    (v128.load32_zero offset=10 align=4 (local.get $0))
  )
  (func (export "v128.load64_zero_offset0") (param $0 i32) (result v128)
    (v128.load64_zero offset=0 (local.get $0))
  )
  (func (export "v128.load64_zero_align1") (param $0 i32) (result v128)
    (v128.load64_zero align=1 (local.get $0))
  )
  (func (export "v128.load64_zero_offset0_align1") (param $0 i32) (result v128)
    (v128.load64_zero offset=0 align=1 (local.get $0))
  )
  (func (export "v128.load64_zero_offset1_align1") (param $0 i32) (result v128)
    (v128.load64_zero offset=1 align=1 (local.get $0))
  )
  (func (export "v128.load64_zero_offset10_align4") (param $0 i32) (result v128)
    (v128.load64_zero offset=10 align=4 (local.get $0))
  )
  (func (export "v128.load64_zero_offset20_align8") (param $0 i32) (result v128)
    (v128.load64_zero offset=20 align=8 (local.get $0))
  )
)


;; normal
(assert_return (invoke "v128.load32_zero" (i32.const 0)) (v128.const i32x4 0x03020100 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load64_zero" (i32.const 0)) (v128.const i64x2 0x0706050403020100 0x0000000000000000))
(assert_return (invoke "v128.load32_zero" (i32.const 10)) (v128.const i32x4 0x0D0C0B0A 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load64_zero" (i32.const 10)) (v128.const i64x2 0x81800F0E0D0C0B0A 0x0000000000000000))
(assert_return (invoke "v128.load32_zero" (i32.const 20)) (v128.const i32x4 0x87868584 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load64_zero" (i32.const 20)) (v128.const i64x2 0x0000898887868584 0x0000000000000000))

;; load by a constant amount
(assert_return (invoke "v128.load32_zero_const0") (v128.const i32x4 0x03020100 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load64_zero_const8") (v128.const i64x2 0x0F0E0D0C0B0A0908 0x0000000000000000))

;; load data with different offset/align arguments
;; load32_zero
(assert_return (invoke "v128.load32_zero_offset0" (i32.const 0)) (v128.const i32x4 0x03020100 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load32_zero_align1" (i32.const 1)) (v128.const i32x4 0x04030201 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load32_zero_offset0_align1" (i32.const 2)) (v128.const i32x4 0x05040302 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load32_zero_offset10_align4" (i32.const 3)) (v128.const i32x4 0x800F0E0D 0x00000000 0x00000000 0x00000000))

;; load64_zero
(assert_return (invoke "v128.load64_zero_offset0" (i32.const 0)) (v128.const i64x2 0x0706050403020100 0x0000000000000000))
(assert_return (invoke "v128.load64_zero_align1" (i32.const 1)) (v128.const i64x2 0x0807060504030201 0x0000000000000000))
(assert_return (invoke "v128.load64_zero_offset0_align1" (i32.const 2)) (v128.const i64x2 0x0908070605040302 0x0000000000000000))
(assert_return (invoke "v128.load64_zero_offset10_align4" (i32.const 3)) (v128.const i64x2 0x84838281800F0E0D 0x0000000000000000))
(assert_return (invoke "v128.load64_zero_offset20_align8" (i32.const 4)) (v128.const i64x2 0x0000000000008988 0x0000000000000000))

;; out of bounds memory access
(assert_trap (invoke "v128.load32_zero" (i32.const -1))  "out of bounds memory access")
(assert_trap (invoke "v128.load64_zero" (i32.const -1))  "out of bounds memory access")

(assert_trap (invoke "v128.load32_zero_offset1_align1" (i32.const -1)) "out of bounds memory access")
(assert_trap (invoke "v128.load64_zero_offset1_align1" (i32.const -1)) "out of bounds memory access")

;; type check
(assert_invalid (module (memory 0) (func (result v128) (v128.load32_zero (f32.const 0)))) "type mismatch")
(assert_invalid (module (memory 0) (func (result v128) (v128.load64_zero (f32.const 0)))) "type mismatch")

;; Test operation with empty argument

(assert_invalid
  (module (memory 0)
    (func $v128.load32_zero-arg-empty (result v128)
      (v128.load32_zero)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module (memory 0)
    (func $v128.load64_zero-arg-empty (result v128)
      (v128.load64_zero)
    )
  )
  "type mismatch"
)

;; Unknown operator

(assert_malformed (module quote "(memory 1) (func (drop (i16x8.load16x4_s (i32.const 0))))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (drop (i16x8.load16x4_u (i32.const 0))))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (drop (i32x4.load32x2_s (i32.const 0))))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (drop (i32x4.load32x2_u (i32.const 0))))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (drop (i64x2.load64x1_s (i32.const 0))))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (drop (i64x2.load64x1_u (i32.const 0))))") "unknown operator")

;; combination
(module
  (memory 1)
  (data (i32.const 0) "\00\01\02\03\04\05\06\07\08\09\0A\0B\0C\0D\0E\0F\80\81\82\83\84\85\86\87\88\89")
  (func (export "v128.load32_zero-in-block") (result v128)
    (block (result v128) (block (result v128) (v128.load32_zero (i32.const 0))))
  )
  (func (export "v128.load64_zero-in-block") (result v128)
    (block (result v128) (block (result v128) (v128.load64_zero (i32.const 1))))
  )
  (func (export "v128.load32_zero-as-br-value") (result v128)
    (block (result v128) (br 0 (v128.load32_zero (i32.const 6))))
  )
  (func (export "v128.load64_zero-as-br-value") (result v128)
    (block (result v128) (br 0 (v128.load64_zero (i32.const 7))))
  )
  (func (export "v128.load32_zero-extract_lane_s-operand") (result i32)
    (i32x4.extract_lane 0 (v128.load32_zero (i32.const 12)))
  )
  (func (export "v128.load64_zero-extract_lane_s-operand") (result i64)
    (i64x2.extract_lane 0 (v128.load64_zero (i32.const 13)))
  )
)
(assert_return (invoke "v128.load32_zero-in-block") (v128.const i32x4 0x03020100 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load64_zero-in-block") (v128.const i64x2 0x0807060504030201 0x0000000000000000))
(assert_return (invoke "v128.load32_zero-as-br-value") (v128.const i32x4 0x09080706 0x00000000 0x00000000 0x00000000))
(assert_return (invoke "v128.load64_zero-as-br-value") (v128.const i64x2 0x0E0D0C0B0A090807 0x0000000000000000))
(assert_return (invoke "v128.load32_zero-extract_lane_s-operand") (i32.const 0x0F0E0D0C))
(assert_return (invoke "v128.load64_zero-extract_lane_s-operand") (i64.const 0x84838281800F0E0D))
