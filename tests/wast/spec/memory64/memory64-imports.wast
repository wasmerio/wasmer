(module (table (export "table-10-inf") 10 funcref))
(register "test-table-10-inf")
(module (table (export "table-10-20") 10 20 funcref))
(register "test-table-10-20")
(module (memory (export "memory-2-inf") 2))
(register "test-memory-2-inf")
(module (memory (export "memory-2-4") 2 4))
(register "test-memory-2-4")

(module (table (export "table64-10-inf") i64 10 funcref))
(register "test-table64-10-inf")
(module (table (export "table64-10-20") i64 10 20 funcref))
(register "test-table64-10-20")
(module (memory (export "memory64-2-inf") i64 2))
(register "test-memory64-2-inf")
(module (memory (export "memory64-2-4") i64 2 4))
(register "test-memory64-2-4")
(module (import "test-table64-10-inf" "table64-10-inf" (table $tab64 i64 10 funcref)))
(module (table $tab64 (import "test-table64-10-inf" "table64-10-inf") i64 10 funcref))
(module (import "test-table64-10-inf" "table64-10-inf" (table i64 10 funcref)))
(module (import "test-table64-10-inf" "table64-10-inf" (table i64 10 funcref)))
(module (table i64 10 funcref))
(module (table i64 10 funcref))
(module (import "test-table64-10-inf" "table64-10-inf" (table i64 10 funcref)))
(module (import "test-table64-10-inf" "table64-10-inf" (table i64 5 funcref)))
(module (import "test-table64-10-inf" "table64-10-inf" (table i64 0 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 10 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 5 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 0 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 10 20 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 5 20 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 0 20 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 10 25 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 5 25 funcref)))
(module (import "test-table64-10-20" "table64-10-20" (table i64 0 25 funcref)))
(assert_unlinkable
  (module (import "test-table64-10-inf" "table64-10-inf" (table i64 12 funcref)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-table64-10-inf" "table64-10-inf" (table i64 10 20 funcref)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-table64-10-20" "table64-10-20" (table i64 12 20 funcref)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-table64-10-20" "table64-10-20" (table i64 10 18 funcref)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-table-10-inf" "table-10-inf" (table i64 10 funcref)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-table64-10-inf" "table64-10-inf" (table 10 funcref)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-table-10-20" "table-10-20" (table i64 10 20 funcref)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-table64-10-20" "table64-10-20" (table 10 20 funcref)))
  "incompatible import type"
)
(module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 2)))
(module (memory (import "test-memory64-2-inf" "memory64-2-inf") i64 2))
(module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 2)))
(module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 1)))
(module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 0)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 2)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 1)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 0)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 2 4)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 1 4)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 0 4)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 2 5)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 1 5)))
(module (import "test-memory64-2-4" "memory64-2-4" (memory i64 0 5)))
(assert_unlinkable
  (module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 0 1)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 0 2)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 0 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 2 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-inf" "memory64-2-inf" (memory i64 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 0 1)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 0 2)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 0 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 2 2)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 2 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 3 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 3 4)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 3 5)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 4 4)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 4 5)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 3)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 4)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory i64 5)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory-2-inf" "memory-2-inf" (memory i64 2)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-inf" "memory64-2-inf" (memory 2)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory-2-4" "memory-2-4" (memory i64 2 4)))
  "incompatible import type"
)
(assert_unlinkable
  (module (import "test-memory64-2-4" "memory64-2-4" (memory 2 4)))
  "incompatible import type"
)
