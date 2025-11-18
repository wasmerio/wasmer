(module
  (func (export "large-sig-no-params")
    (result i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
    (i64.const 1) (i64.const 2) (i64.const 3) (i64.const 4)
    (i64.const 5) (i64.const 6) (i64.const 7) (i64.const 8)
    (i64.const 9) (i64.const 10) (i64.const 11) (i64.const 12)
  )
)

(assert_return
  (invoke "large-sig-no-params")
  (i64.const 1) (i64.const 2) (i64.const 3) (i64.const 4)
  (i64.const 5) (i64.const 6) (i64.const 7) (i64.const 8)
  (i64.const 9) (i64.const 10) (i64.const 11) (i64.const 12)   
)
