;; Code tests taken from
;; https://github.com/WAVM/WAVM/blob/2b919c20a02624af9758e9ddd0b9b5726c973e4f/Test/simd.wast
;;
;; Modified by Wasmer to parse with the wabt spec tests parser and to pass with
;; Wasmer.

;; v128 globals

(module $M
  (global (export "a") v128       (v128.const f32x4 0.0 1.0 2.0 3.0))
  (global (export "b") (mut v128) (v128.const f32x4 4.0 5.0 6.0 7.0))
)
(register "M" $M)

(module
  (global $a (import "M" "a") v128)
  (global $b (import "M" "b") (mut v128))

  (global $c v128       (global.get $a))
  (global $d v128       (v128.const i32x4 8 9 10 11))
  (global $e (mut v128) (global.get $a))
  (global $f (mut v128) (v128.const i32x4 12 13 14 15))

  (func (export "get-a") (result v128) (global.get $a))
  (func (export "get-b") (result v128) (global.get $b))
  (func (export "get-c") (result v128) (global.get $c))
  (func (export "get-d") (result v128) (global.get $d))
  (func (export "get-e") (result v128) (global.get $e))
  (func (export "get-f") (result v128) (global.get $f))

  (func (export "set-b") (param $value v128) (global.set $b (local.get $value)))
  (func (export "set-e") (param $value v128) (global.set $e (local.get $value)))
  (func (export "set-f") (param $value v128) (global.set $f (local.get $value)))
)

(assert_return (invoke "get-a") (v128.const f32x4 0.0 1.0 2.0 3.0))
(assert_return (invoke "get-b") (v128.const f32x4 4.0 5.0 6.0 7.0))
(assert_return (invoke "get-c") (v128.const f32x4 0.0 1.0 2.0 3.0))
(assert_return (invoke "get-d") (v128.const i32x4 8 9 10 11))
(assert_return (invoke "get-e") (v128.const f32x4 0.0 1.0 2.0 3.0))
(assert_return (invoke "get-f") (v128.const i32x4 12 13 14 15))

(invoke "set-b" (v128.const f64x2 nan:0x1 nan:0x2))
(assert_return (invoke "get-b") (v128.const f64x2 nan:0x1 nan:0x2))

(invoke "set-e" (v128.const f64x2 -nan:0x3 +inf))
(assert_return (invoke "get-e") (v128.const f64x2 -nan:0x3 +inf))

(invoke "set-f" (v128.const f32x4 -inf +3.14 10.0e30 +nan:0x42))
(assert_return (invoke "get-f") (v128.const f32x4 -inf +3.14 10.0e30 +nan:0x42))

(assert_invalid (module (global v128 (i32.const 0))) "invalid initializer expression")
(assert_invalid (module (global v128 (i64.const 0))) "invalid initializer expression")
(assert_invalid (module (global v128 (f32.const 0))) "invalid initializer expression")
(assert_invalid (module (global v128 (f64.const 0))) "invalid initializer expression")
(assert_invalid (module (global $i32 i32 (i32.const 0)) (global v128 (global.get $i32))) "invalid initializer expression")

(module binary
  "\00asm"
  "\01\00\00\00"       ;; 1 section
  "\06"                ;; global section
  "\16"                ;; 22 bytes
  "\01"                ;; 1 global
  "\7b"                ;; v128
  "\00"                ;; immutable
  "\fd\02"             ;; v128.const
  "\00\01\02\03"       ;; literal bytes 0-3
  "\04\05\06\07"       ;; literal bytes 4-7
  "\08\09\0a\0b"       ;; literal bytes 8-11
  "\0c\0d\0e\0f"       ;; literal bytes 12-15
  "\0b"                ;; end
)

(assert_invalid
  (module binary
    "\00asm"
    "\01\00\00\00"       ;; 1 section
    "\06\86\80\80\80\00" ;; global section
    "\01"                ;; 1 global
    "\7b"                ;; v128
    "\00"                ;; immutable
    "\fd\00"             ;; v128.load
    "\0b"                ;; end
  )
  "invalid initializer expression"
)

;; TODO: v128 parameters

;; TODO: v128 locals

;; TODO: v128 results

;; v128.const

(module
  (func (export "v128.const/i8x16") (result v128) (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15))
  (func (export "v128.const/i16x8") (result v128) (v128.const i16x8 16 17 18 19 20 21 22 23))
  (func (export "v128.const/i32x4") (result v128) (v128.const i32x4 24 25 26 27))
  (func (export "v128.const/i64x2") (result v128) (v128.const i64x2 28 29))
  (func (export "v128.const/f32x4") (result v128) (v128.const f32x4 30.5 31.5 32.5 33.5))
  (func (export "v128.const/f64x2") (result v128) (v128.const f64x2 34.5 35.5))
)

;; v128.load/store

(module
  (memory 1)
  (func (export "v128.load")  (param $address i32)                     (result v128) (v128.load  align=16 (local.get $address)))
  (func (export "v128.store") (param $address i32) (param $value v128)               (v128.store align=16 (local.get $address) (local.get $value)))
)

(assert_invalid (module (memory 1) (func (drop (v128.load  align=32 (i32.const 0))))) "invalid alignment")
(assert_invalid (module (memory 1) (func (drop (v128.store align=32 (i32.const 0))))) "invalid alignment")

;; *.splat

(module
  (func (export "i8x16.splat") (param $a i32) (result v128) (i8x16.splat (local.get $a)))
  (func (export "i16x8.splat") (param $a i32) (result v128) (i16x8.splat (local.get $a)))
  (func (export "i32x4.splat") (param $a i32) (result v128) (i32x4.splat (local.get $a)))
  (func (export "i64x2.splat") (param $a i64) (result v128) (i64x2.splat (local.get $a)))
  (func (export "f32x4.splat") (param $a f32) (result v128) (f32x4.splat (local.get $a)))
  (func (export "f64x2.splat") (param $a f64) (result v128) (f64x2.splat (local.get $a)))
)

;; *.extract_lane*

(module
  (func (export "i8x16.extract_lane_s/0")  (param $a v128) (result i32) (i8x16.extract_lane_s 0  (local.get $a)))
  (func (export "i8x16.extract_lane_s/1")  (param $a v128) (result i32) (i8x16.extract_lane_s 1  (local.get $a)))
  (func (export "i8x16.extract_lane_s/2")  (param $a v128) (result i32) (i8x16.extract_lane_s 2  (local.get $a)))
  (func (export "i8x16.extract_lane_s/3")  (param $a v128) (result i32) (i8x16.extract_lane_s 3  (local.get $a)))
  (func (export "i8x16.extract_lane_s/4")  (param $a v128) (result i32) (i8x16.extract_lane_s 4  (local.get $a)))
  (func (export "i8x16.extract_lane_s/5")  (param $a v128) (result i32) (i8x16.extract_lane_s 5  (local.get $a)))
  (func (export "i8x16.extract_lane_s/6")  (param $a v128) (result i32) (i8x16.extract_lane_s 6  (local.get $a)))
  (func (export "i8x16.extract_lane_s/7")  (param $a v128) (result i32) (i8x16.extract_lane_s 7  (local.get $a)))
  (func (export "i8x16.extract_lane_s/8")  (param $a v128) (result i32) (i8x16.extract_lane_s 8  (local.get $a)))
  (func (export "i8x16.extract_lane_s/9")  (param $a v128) (result i32) (i8x16.extract_lane_s 9  (local.get $a)))
  (func (export "i8x16.extract_lane_s/10") (param $a v128) (result i32) (i8x16.extract_lane_s 10 (local.get $a)))
  (func (export "i8x16.extract_lane_s/11") (param $a v128) (result i32) (i8x16.extract_lane_s 11 (local.get $a)))
  (func (export "i8x16.extract_lane_s/12") (param $a v128) (result i32) (i8x16.extract_lane_s 12 (local.get $a)))
  (func (export "i8x16.extract_lane_s/13") (param $a v128) (result i32) (i8x16.extract_lane_s 13 (local.get $a)))
  (func (export "i8x16.extract_lane_s/14") (param $a v128) (result i32) (i8x16.extract_lane_s 14 (local.get $a)))
  (func (export "i8x16.extract_lane_s/15") (param $a v128) (result i32) (i8x16.extract_lane_s 15 (local.get $a)))

  (func (export "i8x16.extract_lane_u/0")  (param $a v128) (result i32) (i8x16.extract_lane_u 0  (local.get $a)))
  (func (export "i8x16.extract_lane_u/1")  (param $a v128) (result i32) (i8x16.extract_lane_u 1  (local.get $a)))
  (func (export "i8x16.extract_lane_u/2")  (param $a v128) (result i32) (i8x16.extract_lane_u 2  (local.get $a)))
  (func (export "i8x16.extract_lane_u/3")  (param $a v128) (result i32) (i8x16.extract_lane_u 3  (local.get $a)))
  (func (export "i8x16.extract_lane_u/4")  (param $a v128) (result i32) (i8x16.extract_lane_u 4  (local.get $a)))
  (func (export "i8x16.extract_lane_u/5")  (param $a v128) (result i32) (i8x16.extract_lane_u 5  (local.get $a)))
  (func (export "i8x16.extract_lane_u/6")  (param $a v128) (result i32) (i8x16.extract_lane_u 6  (local.get $a)))
  (func (export "i8x16.extract_lane_u/7")  (param $a v128) (result i32) (i8x16.extract_lane_u 7  (local.get $a)))
  (func (export "i8x16.extract_lane_u/8")  (param $a v128) (result i32) (i8x16.extract_lane_u 8  (local.get $a)))
  (func (export "i8x16.extract_lane_u/9")  (param $a v128) (result i32) (i8x16.extract_lane_u 9  (local.get $a)))
  (func (export "i8x16.extract_lane_u/10") (param $a v128) (result i32) (i8x16.extract_lane_u 10 (local.get $a)))
  (func (export "i8x16.extract_lane_u/11") (param $a v128) (result i32) (i8x16.extract_lane_u 11 (local.get $a)))
  (func (export "i8x16.extract_lane_u/12") (param $a v128) (result i32) (i8x16.extract_lane_u 12 (local.get $a)))
  (func (export "i8x16.extract_lane_u/13") (param $a v128) (result i32) (i8x16.extract_lane_u 13 (local.get $a)))
  (func (export "i8x16.extract_lane_u/14") (param $a v128) (result i32) (i8x16.extract_lane_u 14 (local.get $a)))
  (func (export "i8x16.extract_lane_u/15") (param $a v128) (result i32) (i8x16.extract_lane_u 15 (local.get $a)))

  (func (export "i16x8.extract_lane_s/0")  (param $a v128) (result i32) (i16x8.extract_lane_s 0  (local.get $a)))
  (func (export "i16x8.extract_lane_s/1")  (param $a v128) (result i32) (i16x8.extract_lane_s 1  (local.get $a)))
  (func (export "i16x8.extract_lane_s/2")  (param $a v128) (result i32) (i16x8.extract_lane_s 2  (local.get $a)))
  (func (export "i16x8.extract_lane_s/3")  (param $a v128) (result i32) (i16x8.extract_lane_s 3  (local.get $a)))
  (func (export "i16x8.extract_lane_s/4")  (param $a v128) (result i32) (i16x8.extract_lane_s 4  (local.get $a)))
  (func (export "i16x8.extract_lane_s/5")  (param $a v128) (result i32) (i16x8.extract_lane_s 5  (local.get $a)))
  (func (export "i16x8.extract_lane_s/6")  (param $a v128) (result i32) (i16x8.extract_lane_s 6  (local.get $a)))
  (func (export "i16x8.extract_lane_s/7")  (param $a v128) (result i32) (i16x8.extract_lane_s 7  (local.get $a)))

  (func (export "i16x8.extract_lane_u/0")  (param $a v128) (result i32) (i16x8.extract_lane_u 0  (local.get $a)))
  (func (export "i16x8.extract_lane_u/1")  (param $a v128) (result i32) (i16x8.extract_lane_u 1  (local.get $a)))
  (func (export "i16x8.extract_lane_u/2")  (param $a v128) (result i32) (i16x8.extract_lane_u 2  (local.get $a)))
  (func (export "i16x8.extract_lane_u/3")  (param $a v128) (result i32) (i16x8.extract_lane_u 3  (local.get $a)))
  (func (export "i16x8.extract_lane_u/4")  (param $a v128) (result i32) (i16x8.extract_lane_u 4  (local.get $a)))
  (func (export "i16x8.extract_lane_u/5")  (param $a v128) (result i32) (i16x8.extract_lane_u 5  (local.get $a)))
  (func (export "i16x8.extract_lane_u/6")  (param $a v128) (result i32) (i16x8.extract_lane_u 6  (local.get $a)))
  (func (export "i16x8.extract_lane_u/7")  (param $a v128) (result i32) (i16x8.extract_lane_u 7  (local.get $a)))

  (func (export "i32x4.extract_lane/0")  (param $a v128) (result i32) (i32x4.extract_lane 0  (local.get $a)))
  (func (export "i32x4.extract_lane/1")  (param $a v128) (result i32) (i32x4.extract_lane 1  (local.get $a)))
  (func (export "i32x4.extract_lane/2")  (param $a v128) (result i32) (i32x4.extract_lane 2  (local.get $a)))
  (func (export "i32x4.extract_lane/3")  (param $a v128) (result i32) (i32x4.extract_lane 3  (local.get $a)))

  (func (export "i64x2.extract_lane/0")  (param $a v128) (result i64) (i64x2.extract_lane 0  (local.get $a)))
  (func (export "i64x2.extract_lane/1")  (param $a v128) (result i64) (i64x2.extract_lane 1  (local.get $a)))

  (func (export "f32x4.extract_lane/0")  (param $a v128) (result f32) (f32x4.extract_lane 0  (local.get $a)))
  (func (export "f32x4.extract_lane/1")  (param $a v128) (result f32) (f32x4.extract_lane 1  (local.get $a)))
  (func (export "f32x4.extract_lane/2")  (param $a v128) (result f32) (f32x4.extract_lane 2  (local.get $a)))
  (func (export "f32x4.extract_lane/3")  (param $a v128) (result f32) (f32x4.extract_lane 3  (local.get $a)))

  (func (export "f64x2.extract_lane/0")  (param $a v128) (result f64) (f64x2.extract_lane 0  (local.get $a)))
  (func (export "f64x2.extract_lane/1")  (param $a v128) (result f64) (f64x2.extract_lane 1  (local.get $a)))
)

;; *.replace_lane

(module
  (func (export "i8x16.replace_lane/0")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 0  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/1")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 1  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/2")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 2  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/3")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 3  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/4")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 4  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/5")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 5  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/6")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 6  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/7")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 7  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/8")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 8  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/9")  (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 9  (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/10") (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 10 (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/11") (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 11 (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/12") (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 12 (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/13") (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 13 (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/14") (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 14 (local.get $a) (local.get $b)))
  (func (export "i8x16.replace_lane/15") (param $a v128) (param $b i32) (result v128) (i8x16.replace_lane 15 (local.get $a) (local.get $b)))

  (func (export "i16x8.replace_lane/0")  (param $a v128) (param $b i32) (result v128) (i16x8.replace_lane 0  (local.get $a) (local.get $b)))
  (func (export "i16x8.replace_lane/1")  (param $a v128) (param $b i32) (result v128) (i16x8.replace_lane 1  (local.get $a) (local.get $b)))
  (func (export "i16x8.replace_lane/2")  (param $a v128) (param $b i32) (result v128) (i16x8.replace_lane 2  (local.get $a) (local.get $b)))
  (func (export "i16x8.replace_lane/3")  (param $a v128) (param $b i32) (result v128) (i16x8.replace_lane 3  (local.get $a) (local.get $b)))
  (func (export "i16x8.replace_lane/4")  (param $a v128) (param $b i32) (result v128) (i16x8.replace_lane 4  (local.get $a) (local.get $b)))
  (func (export "i16x8.replace_lane/5")  (param $a v128) (param $b i32) (result v128) (i16x8.replace_lane 5  (local.get $a) (local.get $b)))
  (func (export "i16x8.replace_lane/6")  (param $a v128) (param $b i32) (result v128) (i16x8.replace_lane 6  (local.get $a) (local.get $b)))
  (func (export "i16x8.replace_lane/7")  (param $a v128) (param $b i32) (result v128) (i16x8.replace_lane 7  (local.get $a) (local.get $b)))

  (func (export "i32x4.replace_lane/0")  (param $a v128) (param $b i32) (result v128) (i32x4.replace_lane 0  (local.get $a) (local.get $b)))
  (func (export "i32x4.replace_lane/1")  (param $a v128) (param $b i32) (result v128) (i32x4.replace_lane 1  (local.get $a) (local.get $b)))
  (func (export "i32x4.replace_lane/2")  (param $a v128) (param $b i32) (result v128) (i32x4.replace_lane 2  (local.get $a) (local.get $b)))
  (func (export "i32x4.replace_lane/3")  (param $a v128) (param $b i32) (result v128) (i32x4.replace_lane 3  (local.get $a) (local.get $b)))

  (func (export "i64x2.replace_lane/0")  (param $a v128) (param $b i64) (result v128) (i64x2.replace_lane 0  (local.get $a) (local.get $b)))
  (func (export "i64x2.replace_lane/1")  (param $a v128) (param $b i64) (result v128) (i64x2.replace_lane 1  (local.get $a) (local.get $b)))

  (func (export "f32x4.replace_lane/0")  (param $a v128) (param $b f32) (result v128) (f32x4.replace_lane 0  (local.get $a) (local.get $b)))
  (func (export "f32x4.replace_lane/1")  (param $a v128) (param $b f32) (result v128) (f32x4.replace_lane 1  (local.get $a) (local.get $b)))
  (func (export "f32x4.replace_lane/2")  (param $a v128) (param $b f32) (result v128) (f32x4.replace_lane 2  (local.get $a) (local.get $b)))
  (func (export "f32x4.replace_lane/3")  (param $a v128) (param $b f32) (result v128) (f32x4.replace_lane 3  (local.get $a) (local.get $b)))

  (func (export "f64x2.replace_lane/0")  (param $a v128) (param $b f64) (result v128) (f64x2.replace_lane 0  (local.get $a) (local.get $b)))
  (func (export "f64x2.replace_lane/1")  (param $a v128) (param $b f64) (result v128) (f64x2.replace_lane 1  (local.get $a) (local.get $b)))
)

;; v8x16.swizzle

(module
	(func (export "v8x16.swizzle") (param $elements v128) (param $indices v128) (result v128) (v8x16.swizzle (get_local $elements) (get_local $indices)))
)

(assert_return
	(invoke "v8x16.swizzle"
		(v128.const i8x16 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115)
		(v128.const i8x16  15  14  13  12  11  10   9   8   7   6   5   4   3   2   1   0)
		)
	(v128.const i8x16     115 114 113 112 111 110 109 108 107 106 105 104 103 102 101 100))

(assert_return
	(invoke "v8x16.swizzle"
		(v128.const i8x16 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115)
		(v128.const i8x16  -1   1  -2   2  -3   3  -4   4  -5   5  -6   6  -7   7  -8   8)
		)
	(v128.const i8x16       0 101   0 102   0 103   0 104   0 105   0 106   0 107   0 108))

(assert_return
	(invoke "v8x16.swizzle"
		(v128.const i8x16 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115)
		(v128.const i8x16   9  16  10  17  11  18  12  19  13  20  14  21  15  22  16  23)
		)
	(v128.const i8x16     109   0 110   0 111   0 112   0 113   0 114   0 115   0   0   0))

;; v8x16.shuffle

(module
  (func (export "v8x16.shuffle/0123456789abcdef") (param $a v128) (param $b v128) (result v128) (v8x16.shuffle  0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 (local.get $a) (local.get $b)))
  (func (export "v8x16.shuffle/ghijklmnopqrstuv") (param $a v128) (param $b v128) (result v128) (v8x16.shuffle 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 (local.get $a) (local.get $b)))
  (func (export "v8x16.shuffle/vutsrqponmlkjihg") (param $a v128) (param $b v128) (result v128) (v8x16.shuffle 31 30 29 28 27 26 25 24 23 22 21 20 19 18 17 16 (local.get $a) (local.get $b)))
  (func (export "v8x16.shuffle/fedcba9876543210") (param $a v128) (param $b v128) (result v128) (v8x16.shuffle 15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0 (local.get $a) (local.get $b)))
  (func (export "v8x16.shuffle/0000000000000000") (param $a v128) (param $b v128) (result v128) (v8x16.shuffle  0  0  0  0  0  0  0  0  0  0  0  0  0  0  0  0 (local.get $a) (local.get $b)))
  (func (export "v8x16.shuffle/gggggggggggggggg") (param $a v128) (param $b v128) (result v128) (v8x16.shuffle 16 16 16 16 16 16 16 16 16 16 16 16 16 16 16 16 (local.get $a) (local.get $b)))
  (func (export "v8x16.shuffle/00000000gggggggg") (param $a v128) (param $b v128) (result v128) (v8x16.shuffle  0  0  0  0  0  0  0  0 16 16 16 16 16 16 16 16 (local.get $a) (local.get $b)))
)

;; i*.add

(module
  (func (export "i8x16.add") (param $a v128) (param $b v128) (result v128) (i8x16.add (local.get $a) (local.get $b)))
  (func (export "i16x8.add") (param $a v128) (param $b v128) (result v128) (i16x8.add (local.get $a) (local.get $b)))
  (func (export "i32x4.add") (param $a v128) (param $b v128) (result v128) (i32x4.add (local.get $a) (local.get $b)))
  (func (export "i64x2.add") (param $a v128) (param $b v128) (result v128) (i64x2.add (local.get $a) (local.get $b)))
)

;; i*.sub

(module
  (func (export "i8x16.sub") (param $a v128) (param $b v128) (result v128) (i8x16.sub (local.get $a) (local.get $b)))
  (func (export "i16x8.sub") (param $a v128) (param $b v128) (result v128) (i16x8.sub (local.get $a) (local.get $b)))
  (func (export "i32x4.sub") (param $a v128) (param $b v128) (result v128) (i32x4.sub (local.get $a) (local.get $b)))
  (func (export "i64x2.sub") (param $a v128) (param $b v128) (result v128) (i64x2.sub (local.get $a) (local.get $b)))
)

;; i*.mul

(module
  (func (export "i8x16.mul") (param $a v128) (param $b v128) (result v128) (i8x16.mul (local.get $a) (local.get $b)))
  (func (export "i16x8.mul") (param $a v128) (param $b v128) (result v128) (i16x8.mul (local.get $a) (local.get $b)))
  (func (export "i32x4.mul") (param $a v128) (param $b v128) (result v128) (i32x4.mul (local.get $a) (local.get $b)))
)

;; i*.neg

(module
  (func (export "i8x16.neg") (param $a v128) (result v128) (i8x16.neg (local.get $a)))
  (func (export "i16x8.neg") (param $a v128) (result v128) (i16x8.neg (local.get $a)))
  (func (export "i32x4.neg") (param $a v128) (result v128) (i32x4.neg (local.get $a)))
  (func (export "i64x2.neg") (param $a v128) (result v128) (i64x2.neg (local.get $a)))
)

;; i*.add_saturate*

(module
  (func (export "i8x16.add_saturate_s") (param $a v128) (param $b v128) (result v128) (i8x16.add_saturate_s (local.get $a) (local.get $b)))
  (func (export "i8x16.add_saturate_u") (param $a v128) (param $b v128) (result v128) (i8x16.add_saturate_u (local.get $a) (local.get $b)))
  (func (export "i16x8.add_saturate_s") (param $a v128) (param $b v128) (result v128) (i16x8.add_saturate_s (local.get $a) (local.get $b)))
  (func (export "i16x8.add_saturate_u") (param $a v128) (param $b v128) (result v128) (i16x8.add_saturate_u (local.get $a) (local.get $b)))
)

(assert_return
  (invoke "i8x16.add_saturate_s"
    (v128.const i8x16 127 126 125 124 123 122 121 120 119 120 121 122 123 124 125 126)
    (v128.const i8x16 -7 -6 -5 -4 -3 -2 -1 0 +1 +2 +3 +4 +5 +6 +7 +8))
  (v128.const i8x16 120 120 120 120 120 120 120 120 120 122 124 126 127 127 127 127))

;; i*.sub_saturate*

(module
  (func (export "i8x16.sub_saturate_s") (param $a v128) (param $b v128) (result v128) (i8x16.sub_saturate_s (local.get $a) (local.get $b)))
  (func (export "i8x16.sub_saturate_u") (param $a v128) (param $b v128) (result v128) (i8x16.sub_saturate_u (local.get $a) (local.get $b)))
  (func (export "i16x8.sub_saturate_s") (param $a v128) (param $b v128) (result v128) (i16x8.sub_saturate_s (local.get $a) (local.get $b)))
  (func (export "i16x8.sub_saturate_u") (param $a v128) (param $b v128) (result v128) (i16x8.sub_saturate_u (local.get $a) (local.get $b)))
)

;; v128.and/or/xor/not

(module
  (func (export "v128.and") (param $a v128) (param $b v128) (result v128) (v128.and (local.get $a) (local.get $b)))
  (func (export "v128.or")  (param $a v128) (param $b v128) (result v128) (v128.or  (local.get $a) (local.get $b)))
  (func (export "v128.xor") (param $a v128) (param $b v128) (result v128) (v128.xor (local.get $a) (local.get $b)))
  (func (export "v128.not") (param $a v128)                 (result v128) (v128.not (local.get $a)               ))
)

(module (func (export "v128.bitselect") (param $a v128) (param $b v128) (param $c v128) (result v128) (v128.bitselect (local.get $a) (local.get $b) (local.get $c))))
