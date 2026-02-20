;; Valid cases
(module
  (table $t32 30 30 funcref)
  (table $t64 i64 30 30 funcref)

  (func (export "test32")
    (table.copy $t32 $t32 (i32.const 13) (i32.const 2) (i32.const 3)))

  (func (export "test64")
    (table.copy $t64 $t64 (i64.const 13) (i64.const 2) (i64.const 3)))

  (func (export "test_64to32")
    (table.copy $t32 $t64 (i32.const 13) (i64.const 2) (i32.const 3)))

  (func (export "test_32to64")
    (table.copy $t64 $t32 (i64.const 13) (i32.const 2) (i32.const 3)))
)

;; Invalid cases
(assert_invalid (module
  (table $t32 30 30 funcref)
  (table $t64 i64 30 30 funcref)

  (func (export "bad_size_arg")
    (table.copy $t32 $t64 (i32.const 13) (i64.const 2) (i64.const 3)))
  )
  "type mismatch"
)

(assert_invalid (module
  (table $t32 30 30 funcref)
  (table $t64 i64 30 30 funcref)

  (func (export "bad_src_idx")
    (table.copy $t32 $t64 (i32.const 13) (i32.const 2) (i32.const 3)))
  )
  "type mismatch"
)

(assert_invalid (module
  (table $t32 30 30 funcref)
  (table $t64 i64 30 30 funcref)

  (func (export "bad_dst_idx")
    (table.copy $t32 $t64 (i64.const 13) (i64.const 2) (i32.const 3)))
  )
  "type mismatch"
)
