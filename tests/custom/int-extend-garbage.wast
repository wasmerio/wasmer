;; https://github.com/wasmerio/wasmer/pull/1436
;;
;; When doing an I64ExtendI32U or other integer extension operations, the
;; upper bits in the underlying storage must be cleared.
;;
;; On x86 sign extension is done with its own instruction, `movsx`, so here we only
;; test the unsigned extension case.

(module

  (func (export "i64-extend-i32-u") (result i64)
    ;; fill in stack slots allocated to registers
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))

    ;; push an i64 to produce garbage on the higher 32 bits
    (i64.add (i64.const -1) (i64.const 0))

    ;; pop it
    (drop)

    ;; push an i32
    (i32.add (i32.const 0) (i32.const 0))

    ;; extend
    (i64.extend_i32_u)
    (return)
  )
)

(assert_return (invoke "i64-extend-i32-u") (i64.const 0))
