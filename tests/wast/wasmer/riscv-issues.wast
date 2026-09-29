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

  (func (export "init_atomic_bytes")
    i32.const 0
    i32.const 0x80008000
    i32.store)

  (func (export "atomic_load8_u_sign_bit") (result i32)
    i32.const 1
    i32.atomic.load8_u)

  (func (export "atomic_load16_u_sign_bit") (result i32)
    i32.const 2
    i32.atomic.load16_u)
)

(assert_return (invoke "leu-with-memory") (i32.const 1))

(invoke "init_atomic_bytes")
(assert_return (invoke "atomic_load8_u_sign_bit") (i32.const 128))
(assert_return (invoke "atomic_load16_u_sign_bit") (i32.const 32768))
