(module (memory 1)
  (func (export "leu-with-memory") (result i32)
    (local i32)
    ;; save the constant to memory
    (i32.const 0)
    i32.const -1294967296 ;; 3000000000
    i32.store
    ;; load it from the memory location
    (i32.const 0)
    i32.load
    ;; compare it with a i32 constant
    i32.const -294967296 ;; 4000000000
    i32.le_u
  )
)

(assert_return (invoke "leu-with-memory") (i32.const 1))
