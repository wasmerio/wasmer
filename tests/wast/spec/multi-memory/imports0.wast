(module
  (func (export "func"))
  (func (export "func-i32") (param i32))
  (func (export "func-f32") (param f32))
  (func (export "func->i32") (result i32) (i32.const 22))
  (func (export "func->f32") (result f32) (f32.const 11))
  (func (export "func-i32->i32") (param i32) (result i32) (local.get 0))
  (func (export "func-i64->i64") (param i64) (result i64) (local.get 0))
  (global (export "global-i32") i32 (i32.const 55))
  (global (export "global-f32") f32 (f32.const 44))
  (global (export "global-mut-i64") (mut i64) (i64.const 66))
  (table (export "table-10-inf") 10 funcref)
  (table (export "table-10-20") 10 20 funcref)
  (memory (export "memory-2-inf") 2)
  (memory (export "memory-2-4") 2 4)
)

(register "test")

(assert_unlinkable
  (module (import "test" "memory-2-inf" (func)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test" "memory-2-4" (func)))
  "incompatible import type"
)

(assert_unlinkable
  (module (import "test" "memory-2-inf" (global i32)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test" "memory-2-4" (global i32)))
  "incompatible import type"
)

(assert_unlinkable
  (module (import "test" "memory-2-inf" (table 10 funcref)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test" "memory-2-4" (table 10 funcref)))
  "incompatible import type"
)
