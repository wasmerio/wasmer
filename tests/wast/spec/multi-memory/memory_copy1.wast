;; test memory.copy across different memories.
(module
  (memory $mem0 (data "\ff\11\44\ee"))
  (memory $mem1 (data "\ee\22\55\ff"))
  (memory $mem2 (data "\dd\33\66\00"))
  (memory $mem3 (data "\aa\bb\cc\dd"))

  (func (export "copy") (param i32 i32 i32)
    (memory.copy $mem0 $mem3
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i32) (result i32)
    (i32.load8_u $mem0 (local.get 0)))
)

;; Non-overlapping copy.
(invoke "copy" (i32.const 10) (i32.const 0) (i32.const 4))

(assert_return (invoke "load8_u" (i32.const 9)) (i32.const 0))
(assert_return (invoke "load8_u" (i32.const 10)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i32.const 11)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i32.const 12)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i32.const 13)) (i32.const 0xdd))
(assert_return (invoke "load8_u" (i32.const 14)) (i32.const 0))

;; Copy ending at memory limit is ok.
(invoke "copy" (i32.const 0xff00) (i32.const 0) (i32.const 0x100))
(invoke "copy" (i32.const 0xfe00) (i32.const 0xff00) (i32.const 0x100))

;; Succeed when copying 0 bytes at the end of the region.
(invoke "copy" (i32.const 0x10000) (i32.const 0) (i32.const 0))
(invoke "copy" (i32.const 0) (i32.const 0x10000) (i32.const 0))

;; Copying 0 bytes outside the memory traps.
(assert_trap (invoke "copy" (i32.const 0x10001) (i32.const 0) (i32.const 0))
    "out of bounds memory access")
(assert_trap (invoke "copy" (i32.const 0) (i32.const 0x10001) (i32.const 0))
    "out of bounds memory access")
