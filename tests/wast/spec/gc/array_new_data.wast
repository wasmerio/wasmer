(module
  (type $arr (array (mut i8)))

  (data $d "abcd")

  (func (export "array-new-data") (param i32 i32) (result (ref $arr))
    (array.new_data $arr $d (local.get 0) (local.get 1))
  )
)

;; In-bounds data segment accesses.
(assert_return (invoke "array-new-data" (i32.const 0) (i32.const 0)) (ref.array))
(assert_return (invoke "array-new-data" (i32.const 0) (i32.const 4)) (ref.array))
(assert_return (invoke "array-new-data" (i32.const 1) (i32.const 2)) (ref.array))
(assert_return (invoke "array-new-data" (i32.const 4) (i32.const 0)) (ref.array))

;; Out-of-bounds data segment accesses.
(assert_trap (invoke "array-new-data" (i32.const 0) (i32.const 5)) "out of bounds memory access")
(assert_trap (invoke "array-new-data" (i32.const 5) (i32.const 0)) "out of bounds memory access")
(assert_trap (invoke "array-new-data" (i32.const 1) (i32.const 4)) "out of bounds memory access")
(assert_trap (invoke "array-new-data" (i32.const 4) (i32.const 1)) "out of bounds memory access")

(module
  (type $a32 (array i32))
  (type $a64 (array i64))

  (data $data0 "")
  (data $data1 "1")
  (data $data2 "12")
  (data $data3 "123")
  (data $data4 "1234")
  (data $data7 "1234567")
  (data $data9 "123456789")

  (func (export "f0")
    (drop (array.new_data $a32 $data0 (i32.const 0) (i32.const 1)))
  )
  (func (export "f1")
    (drop (array.new_data $a32 $data1 (i32.const 0) (i32.const 1)))
  )
  (func (export "f2")
    (drop (array.new_data $a32 $data2 (i32.const 0) (i32.const 1)))
  )
  (func (export "f3")
    (drop (array.new_data $a32 $data3 (i32.const 0) (i32.const 1)))
  )
  (func (export "f4")
    (drop (array.new_data $a32 $data4 (i32.const 0) (i32.const 1)))
  )
  (func (export "f9")
    (drop (array.new_data $a32 $data9 (i32.const 6) (i32.const 1)))
  )

  (func (export "g0")
    (drop (array.new_data $a64 $data0 (i32.const 0) (i32.const 1)))
  )
  (func (export "g1")
    (drop (array.new_data $a64 $data1 (i32.const 0) (i32.const 1)))
  )
  (func (export "g4")
    (drop (array.new_data $a64 $data4 (i32.const 0) (i32.const 1)))
  )
  (func (export "g7")
    (drop (array.new_data $a64 $data7 (i32.const 0) (i32.const 1)))
  )
  (func (export "g8")
    (drop (array.new_data $a64 $data9 (i32.const 0) (i32.const 1)))
  )
  (func (export "g9")
    (drop (array.new_data $a64 $data9 (i32.const 2) (i32.const 1)))
  )
)

(assert_trap (invoke "f0") "out of bounds memory access")
(assert_trap (invoke "f1") "out of bounds memory access")
(assert_trap (invoke "f2") "out of bounds memory access")
(assert_trap (invoke "f3") "out of bounds memory access")
(assert_return (invoke "f4"))
(assert_trap (invoke "f9") "out of bounds memory access")

(assert_trap (invoke "g0") "out of bounds memory access")
(assert_trap (invoke "g1") "out of bounds memory access")
(assert_trap (invoke "g4") "out of bounds memory access")
(assert_trap (invoke "g7") "out of bounds memory access")
(assert_return (invoke "g8"))
(assert_trap (invoke "g9") "out of bounds memory access")


(module
  (type $arr (array (mut i8)))

  (data $d "\aa\bb\cc\dd")

  (func (export "array-new-data-contents") (result i32 i32)
    (local (ref $arr))
    (local.set 0 (array.new_data $arr $d (i32.const 1) (i32.const 2)))
    (array.get_u $arr (local.get 0) (i32.const 0))
    (array.get_u $arr (local.get 0) (i32.const 1))
  )
)

;; Array is initialized with the correct contents.
(assert_return (invoke "array-new-data-contents") (i32.const 0xbb) (i32.const 0xcc))

(module
  (type $arr (array (mut i32)))

  (data $d "\aa\bb\cc\dd")

  (func (export "array-new-data-little-endian") (result i32)
    (array.get $arr
               (array.new_data $arr $d (i32.const 0) (i32.const 1))
               (i32.const 0))
  )
)

;; Data segments are interpreted as little-endian.
(assert_return (invoke "array-new-data-little-endian") (i32.const 0xddccbbaa))

(module
  (type $arr (array (mut i16)))

  (data $d "\00\11\22")

  (func (export "array-new-data-unaligned") (result i32)
    (array.get_u $arr
                 (array.new_data $arr $d (i32.const 1) (i32.const 1))
                 (i32.const 0))
  )
)

;; Data inside the segment doesn't need to be aligned to the element size.
(assert_return (invoke "array-new-data-unaligned") (i32.const 0x2211))
