(module
  (memory 1 1 shared)
  ;; [0] = i32(111), [4] = i32(222), [8] = i64(222), [16] = i64(333).
  (data (i32.const 0) "\6f\00\00\00\de\00\00\00\de\00\00\00\00\00\00\00\4d\01\00\00\00\00\00\00")

  (func (export "wait32_via_offset") (result i32)
    (memory.atomic.wait32 offset=4
      (i32.const 0)
      (i32.const 222)
      (i64.const 0)))

  (func (export "wait32_via_index") (result i32)
    (memory.atomic.wait32
      (i32.const 4)
      (i32.const 222)
      (i64.const 0)))

  (func (export "wait64_via_offset") (result i32)
    (memory.atomic.wait64 offset=8
      (i32.const 0)
      (i64.const 222)
      (i64.const 0)))

  (func (export "wait64_via_index") (result i32)
    (memory.atomic.wait64
      (i32.const 8)
      (i64.const 222)
      (i64.const 0)))

  (func (export "wait32_oob_via_offset") (result i32)
    (memory.atomic.wait32 offset=0x10000
      (i32.const 16)
      (i32.const 333)
      (i64.const 0)))

  (func (export "wait64_oob_via_offset") (result i32)
    (memory.atomic.wait64 offset=0x10000
      (i32.const 16)
      (i64.const 333)
      (i64.const 0)))

  (func (export "wait32_addr_overflow") (param i32) (result i32)
    (memory.atomic.wait32 offset=4
      (local.get 0)
      (i32.const 111)
      (i64.const 0)))

  (func (export "wait32_addr_overflow_with_cst") (result i32)
    (memory.atomic.wait32 offset=4
      (i32.const -4)
      (i32.const 111)
      (i64.const 0)))

  (func (export "wait32_unaligned_via_offset") (result i32)
    (memory.atomic.wait32 offset=1
      (i32.const 0)
      (i32.const 111)
      (i64.const 0)))

  (func (export "wait64_unaligned_via_offset") (result i32)
    (memory.atomic.wait64 offset=1
      (i32.const 16)
      (i64.const 333)
      (i64.const 0)))

  (func (export "notify_via_offset") (result i32)
    (memory.atomic.notify offset=4
      (i32.const 0)
      (i32.const 1)))

  (func (export "notify_via_index") (result i32)
    (memory.atomic.notify
      (i32.const 4)
      (i32.const 1)))
)

(assert_return (invoke "wait32_via_offset") (i32.const 2))
(assert_return (invoke "wait32_via_index") (i32.const 2))
(assert_return (invoke "wait64_via_offset") (i32.const 2))
(assert_return (invoke "wait64_via_index") (i32.const 2))

(assert_trap (invoke "wait32_oob_via_offset") "out of bounds memory access")
(assert_trap (invoke "wait64_oob_via_offset") "out of bounds memory access")
(assert_trap (invoke "wait32_addr_overflow_with_cst") "out of bounds memory access")
(assert_trap (invoke "wait32_addr_overflow" (i32.const -4)) "out of bounds memory access")
(assert_trap (invoke "wait32_unaligned_via_offset") "unaligned atomic")
(assert_trap (invoke "wait64_unaligned_via_offset") "unaligned atomic")
(assert_return (invoke "notify_via_offset") (i32.const 0))
(assert_return (invoke "notify_via_index") (i32.const 0))
