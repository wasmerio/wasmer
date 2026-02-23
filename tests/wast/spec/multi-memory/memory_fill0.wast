;; memory.fill
(module
  (memory $mem0 0)
  (memory $mem1 0)
  (memory $mem2 1)

  (func (export "fill") (param i32 i32 i32)
    (memory.fill $mem2
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i32) (result i32)
    (i32.load8_u $mem2 (local.get 0)))
)

;; Basic fill test.
(invoke "fill" (i32.const 1) (i32.const 0xff) (i32.const 3))
(assert_return (invoke "load8_u" (i32.const 0)) (i32.const 0))
(assert_return (invoke "load8_u" (i32.const 1)) (i32.const 0xff))
(assert_return (invoke "load8_u" (i32.const 2)) (i32.const 0xff))
(assert_return (invoke "load8_u" (i32.const 3)) (i32.const 0xff))
(assert_return (invoke "load8_u" (i32.const 4)) (i32.const 0))

;; Fill value is stored as a byte.
(invoke "fill" (i32.const 0) (i32.const 0xbbaa) (i32.const 2))
(assert_return (invoke "load8_u" (i32.const 0)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i32.const 1)) (i32.const 0xaa))

;; Fill all of memory
(invoke "fill" (i32.const 0) (i32.const 0) (i32.const 0x10000))

;; Out-of-bounds writes trap, and nothing is written
(assert_trap (invoke "fill" (i32.const 0xff00) (i32.const 1) (i32.const 0x101))
    "out of bounds memory access")
(assert_return (invoke "load8_u" (i32.const 0xff00)) (i32.const 0))
(assert_return (invoke "load8_u" (i32.const 0xffff)) (i32.const 0))

;; Succeed when writing 0 bytes at the end of the region.
(invoke "fill" (i32.const 0x10000) (i32.const 0) (i32.const 0))

;; Writing 0 bytes outside the memory traps.
(assert_trap (invoke "fill" (i32.const 0x10001) (i32.const 0) (i32.const 0))
    "out of bounds memory access")


