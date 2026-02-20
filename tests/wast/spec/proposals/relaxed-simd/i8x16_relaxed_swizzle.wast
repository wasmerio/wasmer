;; Tests for relaxed i8x16 swizzle.
;; `either` comes from https://github.com/WebAssembly/threads.

(module
    (func (export "i8x16.relaxed_swizzle") (param v128 v128) (result v128) (i8x16.relaxed_swizzle (local.get 0) (local.get 1)))

    (func (export "i8x16.relaxed_swizzle_cmp") (param v128 v128) (result v128)
          (i8x16.eq
            (i8x16.relaxed_swizzle (local.get 0) (local.get 1))
            (i8x16.relaxed_swizzle (local.get 0) (local.get 1))))
)

(assert_return (invoke "i8x16.relaxed_swizzle"
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15))
               (either (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)))

;; out of range, returns 0 or modulo 15 if < 128
(assert_return (invoke "i8x16.relaxed_swizzle"
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31))
               (either (v128.const i8x16 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)))

;; out of range, returns 0 if >= 128
(assert_return (invoke "i8x16.relaxed_swizzle"
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 128 129 130 131 132 133 134 135 248 249 250 251 252 253 254 255))
               (either (v128.const i8x16 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)))

;; Check that multiple calls to the relaxed instruction with same inputs returns same results.

;; out of range, returns 0 or modulo 15 if < 128
(assert_return (invoke "i8x16.relaxed_swizzle_cmp"
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31))
               (v128.const i8x16 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1))

;; out of range, returns 0 if >= 128
(assert_return (invoke "i8x16.relaxed_swizzle_cmp"
                       (v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 128 129 130 131 132 133 134 135 248 249 250 251 252 253 254 255))
               (v128.const i8x16 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1))
