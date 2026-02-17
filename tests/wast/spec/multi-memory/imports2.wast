(module
  (memory (export "z") 0 0)
  (memory (export "memory-2-inf") 2)
  (memory (export "memory-2-4") 2 4)
)

(register "test")

(module
  (import "test" "z" (memory 0))
  (memory $m (import "spectest" "memory") 1 2)
  (data (memory 1) (i32.const 10) "\10")

  (func (export "load") (param i32) (result i32) (i32.load $m (local.get 0)))
)

(assert_return (invoke "load" (i32.const 0)) (i32.const 0))
(assert_return (invoke "load" (i32.const 10)) (i32.const 16))
(assert_return (invoke "load" (i32.const 8)) (i32.const 0x100000))
(assert_trap (invoke "load" (i32.const 1000000)) "out of bounds memory access")

(module
  (memory (import "spectest" "memory") 1 2)
  (data (memory 0) (i32.const 10) "\10")

  (func (export "load") (param i32) (result i32) (i32.load (local.get 0)))
)
(assert_return (invoke "load" (i32.const 0)) (i32.const 0))
(assert_return (invoke "load" (i32.const 10)) (i32.const 16))
(assert_return (invoke "load" (i32.const 8)) (i32.const 0x100000))
(assert_trap (invoke "load" (i32.const 1000000)) "out of bounds memory access")

(module
  (import "test" "memory-2-inf" (memory 2))
  (import "test" "memory-2-inf" (memory 1))
  (import "test" "memory-2-inf" (memory 0))
)

(module
  (import "spectest" "memory" (memory 1))
  (import "spectest" "memory" (memory 0))
  (import "spectest" "memory" (memory 1 2))
  (import "spectest" "memory" (memory 0 2))
  (import "spectest" "memory" (memory 1 3))
  (import "spectest" "memory" (memory 0 3))
)

(assert_unlinkable
  (module (import "test" "unknown" (memory 1)))
  "unknown import"
)
(assert_unlinkable
  (module (import "spectest" "unknown" (memory 1)))
  "unknown import"
)

(assert_unlinkable
  (module (import "test" "memory-2-inf" (memory 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test" "memory-2-inf" (memory 2 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "spectest" "memory" (memory 2)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "spectest" "memory" (memory 1 1)))
  "incompatible import type"
)

