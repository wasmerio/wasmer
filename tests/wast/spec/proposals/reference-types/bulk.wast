;; segment syntax
(module
  (memory 1)
  (data "foo"))

(module
  (table 3 funcref)
  (elem funcref (ref.func 0) (ref.null) (ref.func 1))
  (func)
  (func))

;; memory.fill
(module
  (memory 1)

  (func (export "fill") (param i32 i32 i32)
    (memory.fill
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i32) (result i32)
    (i32.load8_u (local.get 0)))
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


;; memory.copy
(module
  (memory (data "\aa\bb\cc\dd"))

  (func (export "copy") (param i32 i32 i32)
    (memory.copy
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i32) (result i32)
    (i32.load8_u (local.get 0)))
)

;; Non-overlapping copy.
(invoke "copy" (i32.const 10) (i32.const 0) (i32.const 4))

(assert_return (invoke "load8_u" (i32.const 9)) (i32.const 0))
(assert_return (invoke "load8_u" (i32.const 10)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i32.const 11)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i32.const 12)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i32.const 13)) (i32.const 0xdd))
(assert_return (invoke "load8_u" (i32.const 14)) (i32.const 0))

;; Overlap, source > dest
(invoke "copy" (i32.const 8) (i32.const 10) (i32.const 4))
(assert_return (invoke "load8_u" (i32.const 8)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i32.const 9)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i32.const 10)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i32.const 11)) (i32.const 0xdd))
(assert_return (invoke "load8_u" (i32.const 12)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i32.const 13)) (i32.const 0xdd))

;; Overlap, source < dest
(invoke "copy" (i32.const 10) (i32.const 7) (i32.const 6))
(assert_return (invoke "load8_u" (i32.const 10)) (i32.const 0))
(assert_return (invoke "load8_u" (i32.const 11)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i32.const 12)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i32.const 13)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i32.const 14)) (i32.const 0xdd))
(assert_return (invoke "load8_u" (i32.const 15)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i32.const 16)) (i32.const 0))

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


;; memory.init
(module
  (memory 1)
  (data "\aa\bb\cc\dd")

  (func (export "init") (param i32 i32 i32)
    (memory.init 0
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i32) (result i32)
    (i32.load8_u (local.get 0)))
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

;; data.drop
(module
  (memory 1)
  (data $p "x")
  (data $a (memory 0) (i32.const 0) "x")

  (func (export "drop_passive") (data.drop $p))
  (func (export "init_passive") (param $len i32)
    (memory.init $p (i32.const 0) (i32.const 0) (local.get $len)))

  (func (export "drop_active") (data.drop $a))
  (func (export "init_active") (param $len i32)
    (memory.init $a (i32.const 0) (i32.const 0) (local.get $len)))
)

(invoke "init_passive" (i32.const 1))
(invoke "drop_passive")
(invoke "drop_passive")
(assert_return (invoke "init_passive" (i32.const 0)))
(assert_trap (invoke "init_passive" (i32.const 1)) "out of bounds")
(invoke "init_passive" (i32.const 0))
(invoke "drop_active")
(assert_return (invoke "init_active" (i32.const 0)))
(assert_trap (invoke "init_active" (i32.const 1)) "out of bounds")
(invoke "init_active" (i32.const 0))


;; table.init
(module
  (table 3 funcref)
  (elem funcref
    (ref.func $zero) (ref.func $one) (ref.func $zero) (ref.func $one))

  (func $zero (result i32) (i32.const 0))
  (func $one (result i32) (i32.const 1))

  (func (export "init") (param i32 i32 i32)
    (table.init 0
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "call") (param i32) (result i32)
    (call_indirect (result i32)
      (local.get 0)))
)

;; Out-of-bounds stores trap, and nothing is written.
(assert_trap (invoke "init" (i32.const 2) (i32.const 0) (i32.const 2))
    "out of bounds table access")
(assert_trap (invoke "call" (i32.const 2))
    "uninitialized element 2")

(invoke "init" (i32.const 0) (i32.const 1) (i32.const 2))
(assert_return (invoke "call" (i32.const 0)) (i32.const 1))
(assert_return (invoke "call" (i32.const 1)) (i32.const 0))
(assert_trap (invoke "call" (i32.const 2)) "uninitialized element")

;; Init ending at table limit and segment limit is ok.
(invoke "init" (i32.const 1) (i32.const 2) (i32.const 2))

;; Succeed when storing 0 elements at the end of either region.
(invoke "init" (i32.const 3) (i32.const 0) (i32.const 0))
(invoke "init" (i32.const 0) (i32.const 4) (i32.const 0))

;; Writing 0 elements outside the table traps.
(assert_trap (invoke "init" (i32.const 4) (i32.const 0) (i32.const 0))
    "out of bounds table access")
(assert_trap (invoke "init" (i32.const 0) (i32.const 5) (i32.const 0))
    "out of bounds table access")


;; elem.drop
(module
  (table 1 funcref)
  (func $f)
  (elem $p funcref (ref.func $f))
  (elem $a (table 0) (i32.const 0) func $f)

  (func (export "drop_passive") (elem.drop $p))
  (func (export "init_passive") (param $len i32)
    (table.init $p (i32.const 0) (i32.const 0) (local.get $len))
  )

  (func (export "drop_active") (elem.drop $a))
  (func (export "init_active") (param $len i32)
    (table.init $a (i32.const 0) (i32.const 0) (local.get $len))
  )
)

(invoke "init_passive" (i32.const 1))
(invoke "drop_passive")
(invoke "drop_passive")
(assert_return (invoke "init_passive" (i32.const 0)))
(assert_trap (invoke "init_passive" (i32.const 1)) "out of bounds")
(invoke "init_passive" (i32.const 0))
(invoke "drop_active")
(assert_return (invoke "init_active" (i32.const 0)))
(assert_trap (invoke "init_active" (i32.const 1)) "out of bounds")
(invoke "init_active" (i32.const 0))


;; table.copy
(module
  (table 10 funcref)
  (elem (i32.const 0) $zero $one $two)
  (func $zero (result i32) (i32.const 0))
  (func $one (result i32) (i32.const 1))
  (func $two (result i32) (i32.const 2))

  (func (export "copy") (param i32 i32 i32)
    (table.copy
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "call") (param i32) (result i32)
    (call_indirect (result i32)
      (local.get 0)))
)

;; Non-overlapping copy.
(invoke "copy" (i32.const 3) (i32.const 0) (i32.const 3))
;; Now [$zero, $one, $two, $zero, $one, $two, ...]
(assert_return (invoke "call" (i32.const 3)) (i32.const 0))
(assert_return (invoke "call" (i32.const 4)) (i32.const 1))
(assert_return (invoke "call" (i32.const 5)) (i32.const 2))

;; Overlap, source > dest
(invoke "copy" (i32.const 0) (i32.const 1) (i32.const 3))
;; Now [$one, $two, $zero, $zero, $one, $two, ...]
(assert_return (invoke "call" (i32.const 0)) (i32.const 1))
(assert_return (invoke "call" (i32.const 1)) (i32.const 2))
(assert_return (invoke "call" (i32.const 2)) (i32.const 0))

;; Overlap, source < dest
(invoke "copy" (i32.const 2) (i32.const 0) (i32.const 3))
;; Now [$one, $two, $one, $two, $zero, $two, ...]
(assert_return (invoke "call" (i32.const 2)) (i32.const 1))
(assert_return (invoke "call" (i32.const 3)) (i32.const 2))
(assert_return (invoke "call" (i32.const 4)) (i32.const 0))

;; Copy ending at table limit is ok.
(invoke "copy" (i32.const 6) (i32.const 8) (i32.const 2))
(invoke "copy" (i32.const 8) (i32.const 6) (i32.const 2))

;; Succeed when copying 0 elements at the end of the region.
(invoke "copy" (i32.const 10) (i32.const 0) (i32.const 0))
(invoke "copy" (i32.const 0) (i32.const 10) (i32.const 0))

;; Fail on out-of-bounds when copying 0 elements outside of table.
(assert_trap (invoke "copy" (i32.const 11) (i32.const 0) (i32.const 0))
  "out of bounds")
(assert_trap (invoke "copy" (i32.const 0) (i32.const 11) (i32.const 0))
  "out of bounds")
