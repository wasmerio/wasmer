;; Tests for i8x16.relaxed_laneselect, i16x8.relaxed_laneselect, i32x4.relaxed_laneselect, and i64x2.relaxed_laneselect.

(module
    (func (export "i8x16.relaxed_laneselect") (param v128 v128 v128) (result v128) (i8x16.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2)))
    (func (export "i16x8.relaxed_laneselect") (param v128 v128 v128) (result v128) (i16x8.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2)))
    (func (export "i32x4.relaxed_laneselect") (param v128 v128 v128) (result v128) (i32x4.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2)))
    (func (export "i64x2.relaxed_laneselect") (param v128 v128 v128) (result v128) (i64x2.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2)))

    (func (export "i8x16.relaxed_laneselect_cmp") (param v128 v128 v128) (result v128)
          (i8x16.eq
            (i8x16.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2))
            (i8x16.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2))))
    (func (export "i16x8.relaxed_laneselect_cmp") (param v128 v128 v128) (result v128)
          (i16x8.eq
            (i16x8.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2))
            (i16x8.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2))))
    (func (export "i32x4.relaxed_laneselect_cmp") (param v128 v128 v128) (result v128)
          (i32x4.eq
            (i32x4.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2))
            (i32x4.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2))))
    (func (export "i64x2.relaxed_laneselect_cmp") (param v128 v128 v128) (result v128)
          (i64x2.eq
            (i64x2.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2))
            (i64x2.relaxed_laneselect (local.get 0) (local.get 1) (local.get 2))))
)

(assert_return (invoke "i8x16.relaxed_laneselect"
                       (v128.const i8x16 0    1  0x12 0x12 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 16   17 0x34 0x34 20 21 22 23 24 25 26 27 28 29 30 31)
                       (v128.const i8x16 0xff 0  0xf0 0x0f 0 0 0 0 0 0 0 0 0 0 0 0))
               (either (v128.const i8x16 0    17 0x14 0x32 20 21 22 23 24 25 26 27 28 29 30 31)
                       (v128.const i8x16 0    17 0x12 0x34 20 21 22 23 24 25 26 27 28 29 30 31)))

(assert_return (invoke "i16x8.relaxed_laneselect"
                       (v128.const i16x8 0      1 0x1234 0x1234 4 5 6 7)
                       (v128.const i16x8 8      9 0x5678 0x5678 12 13 14 15)
                       (v128.const i16x8 0xffff 0 0xff00 0x00ff 0 0 0 0))
               (either (v128.const i16x8 0      9 0x1278 0x5634 12 13 14 15)
                       (v128.const i16x8 0      9 0x1234 0x5678 12 13 14 15)))

;; special case for i16x8 to allow pblendvb
(assert_return (invoke "i16x8.relaxed_laneselect"
                       (v128.const i16x8 0      1 0x1234 0x1234 4 5 6 7)
                       (v128.const i16x8 8      9 0x5678 0x5678 12 13 14 15)
                       (v128.const i16x8 0xffff 0 0xff00 0x0080 0 0 0 0))  ;; 0x0080 is the special case
               (either (v128.const i16x8 0      9 0x1278 0x5678 12 13 14 15)  ;; bitselect
                       (v128.const i16x8 0      9 0x1234 0x5678 12 13 14 15)  ;; top bit of i16 lane examined
                       (v128.const i16x8 0      9 0x1278 0x5634 12 13 14 15)  ;; top bit of each byte
                       ))

(assert_return (invoke "i32x4.relaxed_laneselect"
                       (v128.const i32x4 0          1 0x12341234 0x12341234)
                       (v128.const i32x4 4          5 0x56785678 0x56785678)
                       (v128.const i32x4 0xffffffff 0 0xffff0000 0x0000ffff))
               (either (v128.const i32x4 0          5 0x12345678 0x56781234)
                       (v128.const i32x4 0          5 0x12341234 0x56785678)))

(assert_return (invoke "i64x2.relaxed_laneselect"
                       (v128.const i64x2 0                  1)
                       (v128.const i64x2 2                  3)
                       (v128.const i64x2 0xffffffffffffffff 0))
               (either (v128.const i64x2 0                  3)
                       (v128.const i64x2 0                  3)))

(assert_return (invoke "i64x2.relaxed_laneselect"
                       (v128.const i64x2 0x1234123412341234 0x1234123412341234)
                       (v128.const i64x2 0x5678567856785678 0x5678567856785678)
                       (v128.const i64x2 0xffffffff00000000 0x00000000ffffffff))
               (either (v128.const i64x2 0x1234123456785678 0x5678567812341234)
                       (v128.const i64x2 0x1234123412341234 0x5678567856785678)))

;; Check that multiple calls to the relaxed instruction with same inputs returns same results.

(assert_return (invoke "i8x16.relaxed_laneselect_cmp"
                       (v128.const i8x16 0    1  0x12 0x12 4 5 6 7 8 9 10 11 12 13 14 15)
                       (v128.const i8x16 16   17 0x34 0x34 20 21 22 23 24 25 26 27 28 29 30 31)
                       (v128.const i8x16 0xff 0  0xf0 0x0f 0 0 0 0 0 0 0 0 0 0 0 0))
               (v128.const i8x16 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1 -1))

(assert_return (invoke "i16x8.relaxed_laneselect_cmp"
                       (v128.const i16x8 0      1 0x1234 0x1234 4 5 6 7)
                       (v128.const i16x8 8      9 0x5678 0x5678 12 13 14 15)
                       (v128.const i16x8 0xffff 0 0xff00 0x00ff 0 0 0 0))
               (v128.const i16x8 -1 -1 -1 -1 -1 -1 -1 -1))

(assert_return (invoke "i32x4.relaxed_laneselect_cmp"
                       (v128.const i32x4 0          1 0x12341234 0x12341234)
                       (v128.const i32x4 4          5 0x56785678 0x56785678)
                       (v128.const i32x4 0xffffffff 0 0xffff0000 0x0000ffff))
               (v128.const i32x4 -1 -1 -1 -1))

(assert_return (invoke "i64x2.relaxed_laneselect_cmp"
                       (v128.const i64x2 0                  1)
                       (v128.const i64x2 2                  3)
                       (v128.const i64x2 0xffffffffffffffff 0))
               (v128.const i64x2 -1 -1))

(assert_return (invoke "i64x2.relaxed_laneselect_cmp"
                       (v128.const i64x2 0x1234123412341234 0x1234123412341234)
                       (v128.const i64x2 0x5678567856785678 0x5678567856785678)
                       (v128.const i64x2 0xffffffff00000000 0x00000000ffffffff))
               (v128.const i64x2 -1 -1))
