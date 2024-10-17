;; v128.load operater with normal argument (e.g. (i8x16, i16x8 i32x4))

(module
  (memory 1)
  (data (i32.const 0) "\00\01\02\03\04\05\06\07\08\09\0a\0b\0c\0d\0e\0f\00\01\02\03")
  (func (export "v128.load") (result v128)
    (v128.load (i32.const 0))
  )
)

(assert_return (invoke "v128.load") (v128.const i8x16 0x00 0x01 0x02 0x03 0x04 0x05 0x06 0x07 0x08 0x09 0x0a 0x0b 0x0c 0x0d 0x0e 0x0f))
(assert_return (invoke "v128.load") (v128.const i16x8 0x0100 0x0302 0x0504 0x0706 0x0908 0x0b0a 0x0d0c 0x0f0e))
(assert_return (invoke "v128.load") (v128.const i32x4 0x03020100 0x07060504 0x0b0a0908 0x0f0e0d0c))


;; v128.load operater as the argument of other SIMD instructions

(module (memory 1)
  (data (i32.const 0) "\00\01\02\03\04\05\06\07\08\09\0a\0b\0c\0d\0e\0f\00\01\02\03")
  (func (export "as-i8x16_extract_lane_s-value/0") (result i32)
    (i8x16.extract_lane_s 0 (v128.load (i32.const 0)))
  )
)
(assert_return (invoke "as-i8x16_extract_lane_s-value/0") (i32.const 0x00))

(module (memory 1)
  (data (i32.const 0) "\00\01\02\03\04\05\06\07\08\09\0a\0b\0c\0d\0e\0f\00\01\02\03")
  (func (export "as-i8x16.eq-operand") (result v128)
    (i8x16.eq (v128.load offset=0 (i32.const 0)) (v128.load offset=16 (i32.const 0)))
  )
)
(assert_return (invoke "as-i8x16.eq-operand") (v128.const i32x4 0xffffffff 0x00000000 0x00000000 0x00000000))

(module (memory 1)
  (data (i32.const 0) "\00\01\02\03\04\05\06\07\08\09\0a\0b\0c\0d\0e\0f\00\01\02\03")
  (func (export "as-v128.not-operand") (result v128)
    (v128.not (v128.load (i32.const 0)))
  )
  (func (export "as-i8x16.all_true-operand") (result i32)
    (i8x16.all_true (v128.load (i32.const 0)))
  )
)
(assert_return (invoke "as-v128.not-operand") (v128.const i32x4 0xfcfdfeff 0xf8f9fafb 0xf4f5f6f7 0xf0f1f2f3))
(assert_return (invoke "as-i8x16.all_true-operand") (i32.const 0))

(module (memory 1)
  (data (offset (i32.const 0))  "\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA")
  (data (offset (i32.const 16)) "\BB\BB\BB\BB\BB\BB\BB\BB\BB\BB\BB\BB\BB\BB\BB\BB")
  (data (offset (i32.const 32)) "\F0\F0\F0\F0\FF\FF\FF\FF\00\00\00\00\FF\00\FF\00")
  (func (export "as-v128.bitselect-operand") (result v128)
    (v128.bitselect (v128.load (i32.const 0)) (v128.load (i32.const 16)) (v128.load (i32.const 32)))
  )
)
(assert_return (invoke "as-v128.bitselect-operand") (v128.const i32x4 0xabababab 0xaaaaaaaa 0xbbbbbbbb 0xbbaabbaa))

(module (memory 1)
  (data (offset (i32.const 0)) "\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA")
  (func (export "as-i8x16.shl-operand") (result v128)
    (i8x16.shl (v128.load (i32.const 0)) (i32.const 1))
  )
)
(assert_return (invoke "as-i8x16.shl-operand") (v128.const i32x4 0x54545454 0x54545454 0x54545454 0x54545454)) ;; 1010 1000 << 1010 1010

(module (memory 1)
  (data (offset (i32.const 0))  "\02\00\00\00\02\00\00\00\02\00\00\00\02\00\00\00")
  (data (offset (i32.const 16)) "\03\00\00\00\03\00\00\00\03\00\00\00\03\00\00\00")
  (func (export "as-add/sub-operand") (result v128)
    ;; 2 2 2 2 + 3 3 3 3 = 5 5 5 5
    ;; 5 5 5 5 - 3 3 3 3 = 2 2 2 2
    (i8x16.sub
      (i8x16.add (v128.load (i32.const 0)) (v128.load (i32.const 16)))
      (v128.load (i32.const 16))
    )
  )
)
(assert_return (invoke "as-add/sub-operand") (v128.const i32x4 2 2 2 2))

(module (memory 1)
  (data (offset (i32.const 0))  "\00\00\00\43\00\00\80\3f\66\66\e6\3f\00\00\80\bf")  ;; 128 1.0 1.8 -1
  (data (offset (i32.const 16)) "\00\00\00\40\00\00\00\40\00\00\00\40\00\00\00\40")  ;; 2.0 2.0 2.0 2.0
  (func (export "as-f32x4.mul-operand") (result v128)
    (f32x4.mul (v128.load (i32.const 0)) (v128.load (i32.const 16)))
  )
)
(assert_return (invoke "as-f32x4.mul-operand") (v128.const f32x4 256 2 3.6 -2))

(module (memory 1)
  (data (offset (i32.const 0)) "\ff\ff\ff\ff\ff\ff\ff\ff\ff\ff\ff\ff\ff\ff\ff\ff")  ;; 1111 ...
  (func (export "as-f32x4.abs-operand") (result v128)
    (f32x4.abs (v128.load (i32.const 0)))
  )
)
(assert_return (invoke "as-f32x4.abs-operand") (v128.const i32x4 0x7fffffff 0x7fffffff 0x7fffffff 0x7fffffff)) ;; 1111 -> 0111

(module (memory 1)
  (data (offset (i32.const 0)) "\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA\AA")
  (data (offset (i32.const 16)) "\02\00\00\00\02\00\00\00\02\00\00\00\02\00\00\00")
  (func (export "as-f32x4.min-operand") (result v128)
    (f32x4.min (v128.load (i32.const 0)) (v128.load offset=16 (i32.const 1)))
  )
)
(assert_return (invoke "as-f32x4.min-operand") (v128.const i32x4 0xaaaaaaaa 0xaaaaaaaa 0xaaaaaaaa 0xaaaaaaaa)) ;; signed 1010 < 0010

(module (memory 1)
  (data (offset (i32.const 0))  "\00\00\00\43\00\00\80\3f\66\66\e6\3f\00\00\80\bf")  ;; 128 1.0 1.8 -1
  (func (export "as-i32x4.trunc_sat_f32x4_s-operand") (result v128)
    (i32x4.trunc_sat_f32x4_s (v128.load (i32.const 0)))
  )
)
(assert_return (invoke "as-i32x4.trunc_sat_f32x4_s-operand") (v128.const i32x4 128 1 1 -1)) ;; 128 1.0 1.8 -1 -> 128 1 1 -1

(module (memory 1)
  (data (offset (i32.const 0)) "\02\00\00\00\02\00\00\00\02\00\00\00\02\00\00\00")
  (func (export "as-f32x4.convert_i32x4_u-operand") (result v128)
    (f32x4.convert_i32x4_u (v128.load (i32.const 0)))
  )
)
(assert_return (invoke "as-f32x4.convert_i32x4_u-operand") (v128.const f32x4 2 2 2 2))

(module (memory 1)
  (data (offset (i32.const 0)) "\64\65\66\67\68\69\6a\6b\6c\6d\6e\6f\70\71\72\73")  ;; 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115
  (data (offset (i32.const 16)) "\0f\0e\0d\0c\0b\0a\09\08\07\06\05\04\03\02\01\00")  ;;  15  14  13  12  11  10  09  08  07  06  05  04  03  02  01  00
  (func (export "as-i8x16.swizzle-operand") (result v128)
    (i8x16.swizzle (v128.load (i32.const 0)) (v128.load offset=15 (i32.const 1)))
  )
)
(assert_return(invoke "as-i8x16.swizzle-operand") (v128.const i8x16 115 114 113 112 111 110 109 108 107 106 105 104 103 102 101 100))

(module (memory 1)
  (data (i32.const 0) "\00\01\02\03\04\05\06\07\08\09\0a\0b\0c\0d\0e\0f\00\01\02\03")
  (func (export "as-br-value") (result v128)
    (block (result v128) (br 0 (v128.load (i32.const 0))))
  )
)
(assert_return (invoke "as-br-value") (v128.const i32x4 0x03020100 0x07060504 0x0b0a0908 0x0f0e0d0c))


;; Unknown operator(e.g. v128.load8, v128.load16, v128.load32)

(assert_malformed
  (module quote
    "(memory 1)"
    "(func (local v128) (drop (v128.load8 (i32.const 0))))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1)"
    "(func (local v128) (drop (v128.load16 (i32.const 0))))"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(memory 1)"
    "(func (local v128) (drop (v128.load32 (i32.const 0))))"
  )
  "unknown operator"
)


;; Type mismatched (e.g. v128.load(f32.const 0), type address empty)

(assert_invalid
  (module (memory 1) (func (local v128) (drop (v128.load (f32.const 0)))))
  "type mismatch"
)
(assert_invalid
  (module (memory 1) (func (local v128) (block (br_if 0 (v128.load (i32.const 0))))))
  "type mismatch"
)
(assert_invalid
  (module (memory 1) (func (local v128) (v128.load (i32.const 0))))
  "type mismatch"
)


;; Type address empty

(assert_invalid
  (module (memory 1) (func (drop (v128.load (local.get 2)))))
  "unknown local 2"
)
(assert_invalid
  (module (memory 1) (func (drop (v128.load))))
  "type mismatch"
)