;; Test SIMD select instuction

(module
  (func (export "select_v128_i32") (param v128 v128 i32) (result v128)
    (select (local.get 0) (local.get 1) (local.get 2))
  )
)

(assert_return
  (invoke "select_v128_i32"
    (v128.const i32x4 1 2 3 4)
    (v128.const i32x4 5 6 7 8)
    (i32.const 1)
  )
  (v128.const i32x4 1 2 3 4)
)

(assert_return
  (invoke "select_v128_i32"
    (v128.const i32x4 1 2 3 4)
    (v128.const i32x4 5 6 7 8)
    (i32.const 0)
  )
  (v128.const i32x4 5 6 7 8)
)

(assert_return
  (invoke "select_v128_i32"
    (v128.const f32x4 1.0 2.0 3.0 4.0)
    (v128.const f32x4 5.0 6.0 7.0 8.0)
    (i32.const -1)
  )
  (v128.const f32x4 1.0 2.0 3.0 4.0)
)

(assert_return
  (invoke "select_v128_i32"
    (v128.const f32x4 -1.5 -2.5 -3.5 -4.5)
    (v128.const f32x4 9.5 8.5 7.5 6.5)
    (i32.const 0)
  )
  (v128.const f32x4 9.5 8.5 7.5 6.5)
)

(assert_return
  (invoke "select_v128_i32"
    (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
    (v128.const i8x16 16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1)
    (i32.const 123)
  )
  (v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
)

(assert_return
  (invoke "select_v128_i32"
    (v128.const i8x16 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1)
    (v128.const i8x16 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
    (i32.const 0)
  )
  (v128.const i8x16 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)
)
