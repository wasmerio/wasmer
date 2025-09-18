;; This doesn't test anything in particular; this is just something
;; that was broken in our LLVM backend at one point.

(module
  (func (export "f") (result i32)
    i32.const 1
    i32.const 2
    (block $outer (param i32) (result i32)
      (block (param i32) (result i32)
        br $outer
      )
      unreachable
    )
    i32.add
  )
)

(assert_return (invoke "f") (i32.const 3))