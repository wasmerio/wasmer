;; memory.init
(module
  (memory $mem0 0)
  (memory $mem1 0)
  (memory $mem2 1)
  (memory $mem3 0)
  (data $mem2 "\aa\bb\cc\dd")

  (func (export "init") (param i32 i32 i32)
    (memory.init $mem2 0
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i32) (result i32)
    (i32.load8_u $mem2 (local.get 0)))
)

(invoke "init" (i32.const 0) (i32.const 1) (i32.const 2))
(assert_return (invoke "load8_u" (i32.const 0)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i32.const 1)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i32.const 2)) (i32.const 0))

;; Init ending at memory limit and segment limit is ok.
(invoke "init" (i32.const 0xfffc) (i32.const 0) (i32.const 4))

;; Out-of-bounds writes trap, and nothing is written.
(assert_trap (invoke "init" (i32.const 0xfffe) (i32.const 0) (i32.const 3))
    "out of bounds memory access")
(assert_return (invoke "load8_u" (i32.const 0xfffe)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i32.const 0xffff)) (i32.const 0xdd))

;; Succeed when writing 0 bytes at the end of either region.
(invoke "init" (i32.const 0x10000) (i32.const 0) (i32.const 0))
(invoke "init" (i32.const 0) (i32.const 4) (i32.const 0))

;; Writing 0 bytes outside the memory traps.
(assert_trap (invoke "init" (i32.const 0x10001) (i32.const 0) (i32.const 0))
    "out of bounds memory access")
(assert_trap (invoke "init" (i32.const 0) (i32.const 5) (i32.const 0))
    "out of bounds memory access")

