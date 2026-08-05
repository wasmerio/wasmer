(module
  ;; Entry 0 branches to the innermost label.
  (func (export "index_0") (result i32)
    (block $outer
      (block $middle
        (block $inner
          i32.const 0
          br_table $inner $middle $outer
        )
        i32.const 10
        return
      )
      i32.const 20
      return
    )
    i32.const 30
  )

  ;; Entry 1 branches to the middle label.
  (func (export "index_1") (result i32)
    (block $outer
      (block $middle
        (block $inner
          i32.const 1
          br_table $inner $middle $outer
        )
        i32.const 10
        return
      )
      i32.const 20
      return
    )
    i32.const 30
  )

  ;; Index 2 is just past the two table entries, so it uses the default.
  (func (export "index_2") (result i32)
    (block $outer
      (block $middle
        (block $inner
          i32.const 2
          br_table $inner $middle $outer
        )
        i32.const 10
        return
      )
      i32.const 20
      return
    )
    i32.const 30
  )

  ;; A larger positive index also uses the default.
  (func (export "index_42") (result i32)
    (block $outer
      (block $middle
        (block $inner
          i32.const 42
          br_table $inner $middle $outer
        )
        i32.const 10
        return
      )
      i32.const 20
      return
    )
    i32.const 30
  )

  ;; br_table treats the index as unsigned, so -1 uses the default.
  (func (export "index_neg_1") (result i32)
    (block $outer
      (block $middle
        (block $inner
          i32.const -1
          br_table $inner $middle $outer
        )
        i32.const 10
        return
      )
      i32.const 20
      return
    )
    i32.const 30
  )
)

(assert_return (invoke "index_0") (i32.const 10))
(assert_return (invoke "index_1") (i32.const 20))
(assert_return (invoke "index_2") (i32.const 30))
(assert_return (invoke "index_42") (i32.const 30))
(assert_return (invoke "index_neg_1") (i32.const 30))
