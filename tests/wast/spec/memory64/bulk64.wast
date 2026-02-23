;; segment syntax
(module
  (memory i64 1)
  (data "foo"))

;; memory.fill
(module
  (memory i64 1)

  (func (export "fill") (param i64 i32 i64)
    (memory.fill
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i64) (result i32)
    (i32.load8_u (local.get 0)))
)

;; Basic fill test.
(invoke "fill" (i64.const 1) (i32.const 0xff) (i64.const 3))
(assert_return (invoke "load8_u" (i64.const 0)) (i32.const 0))
(assert_return (invoke "load8_u" (i64.const 1)) (i32.const 0xff))
(assert_return (invoke "load8_u" (i64.const 2)) (i32.const 0xff))
(assert_return (invoke "load8_u" (i64.const 3)) (i32.const 0xff))
(assert_return (invoke "load8_u" (i64.const 4)) (i32.const 0))

;; Fill value is stored as a byte.
(invoke "fill" (i64.const 0) (i32.const 0xbbaa) (i64.const 2))
(assert_return (invoke "load8_u" (i64.const 0)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i64.const 1)) (i32.const 0xaa))

;; Fill all of memory
(invoke "fill" (i64.const 0) (i32.const 0) (i64.const 0x10000))

;; Succeed when writing 0 bytes at the end of the region.
(invoke "fill" (i64.const 0x10000) (i32.const 0) (i64.const 0))

;; Writing 0 bytes outside of memory limit is NOT allowed.
(assert_trap
  (invoke "fill" (i64.const 0x10001) (i32.const 0) (i64.const 0))
  "out of bounds memory access")

;; memory.copy
(module
  (memory i64 1 1)
  (data (i64.const 0) "\aa\bb\cc\dd")

  (func (export "copy") (param i64 i64 i64)
    (memory.copy
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i64) (result i32)
    (i32.load8_u (local.get 0)))
)

;; Non-overlapping copy.
(invoke "copy" (i64.const 10) (i64.const 0) (i64.const 4))

(assert_return (invoke "load8_u" (i64.const 9)) (i32.const 0))
(assert_return (invoke "load8_u" (i64.const 10)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i64.const 11)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i64.const 12)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 13)) (i32.const 0xdd))
(assert_return (invoke "load8_u" (i64.const 14)) (i32.const 0))

;; Overlap, source > dest
(invoke "copy" (i64.const 8) (i64.const 10) (i64.const 4))
(assert_return (invoke "load8_u" (i64.const 8)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i64.const 9)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i64.const 10)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 11)) (i32.const 0xdd))
(assert_return (invoke "load8_u" (i64.const 12)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 13)) (i32.const 0xdd))

;; Overlap, source < dest
(invoke "copy" (i64.const 10) (i64.const 7) (i64.const 6))
(assert_return (invoke "load8_u" (i64.const 10)) (i32.const 0))
(assert_return (invoke "load8_u" (i64.const 11)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i64.const 12)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i64.const 13)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 14)) (i32.const 0xdd))
(assert_return (invoke "load8_u" (i64.const 15)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 16)) (i32.const 0))

;; Overlap, source < dest but size is out of bounds
(assert_trap
  (invoke "copy" (i64.const 13) (i64.const 11) (i64.const -1))
   "out of bounds memory access")
(assert_return (invoke "load8_u" (i64.const 10)) (i32.const 0))
(assert_return (invoke "load8_u" (i64.const 11)) (i32.const 0xaa))
(assert_return (invoke "load8_u" (i64.const 12)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i64.const 13)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 14)) (i32.const 0xdd))
(assert_return (invoke "load8_u" (i64.const 15)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 16)) (i32.const 0))

;; Copy ending at memory limit is ok.
(invoke "copy" (i64.const 0xff00) (i64.const 0) (i64.const 0x100))
(invoke "copy" (i64.const 0xfe00) (i64.const 0xff00) (i64.const 0x100))

;; Succeed when copying 0 bytes at the end of the region.
(invoke "copy" (i64.const 0x10000) (i64.const 0) (i64.const 0))
(invoke "copy" (i64.const 0) (i64.const 0x10000) (i64.const 0))

;; Copying 0 bytes outside of memory limit is NOT allowed.
(assert_trap
  (invoke "copy" (i64.const 0x10001) (i64.const 0) (i64.const 0))
  "out of bounds memory access")
(assert_trap
  (invoke "copy" (i64.const 0) (i64.const 0x10001) (i64.const 0))
  "out of bounds memory access")

;; memory.init
(module
  (memory i64 1)
  (data "\aa\bb\cc\dd")

  (func (export "init") (param i64 i32 i32)
    (memory.init 0
      (local.get 0)
      (local.get 1)
      (local.get 2)))

  (func (export "load8_u") (param i64) (result i32)
    (i32.load8_u (local.get 0)))
)

(invoke "init" (i64.const 0) (i32.const 1) (i32.const 2))
(assert_return (invoke "load8_u" (i64.const 0)) (i32.const 0xbb))
(assert_return (invoke "load8_u" (i64.const 1)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 2)) (i32.const 0))

;; Init ending at memory limit and segment limit is ok.
(invoke "init" (i64.const 0xfffc) (i32.const 0) (i32.const 4))

;; Out-of-bounds writes trap, and no partial writes has been made.
(assert_trap (invoke "init" (i64.const 0xfffe) (i32.const 0) (i32.const 3))
    "out of bounds memory access")
(assert_return (invoke "load8_u" (i64.const 0xfffe)) (i32.const 0xcc))
(assert_return (invoke "load8_u" (i64.const 0xffff)) (i32.const 0xdd))

;; Succeed when writing 0 bytes at the end of either region.
(invoke "init" (i64.const 0x10000) (i32.const 0) (i32.const 0))
(invoke "init" (i64.const 0) (i32.const 4) (i32.const 0))

;; Writing 0 bytes outside of memory / segment limit is NOT allowed.
(assert_trap
  (invoke "init" (i64.const 0x10001) (i32.const 0) (i32.const 0))
  "out of bounds memory access")
(assert_trap
  (invoke "init" (i64.const 0) (i32.const 5) (i32.const 0))
  "out of bounds memory access")

;; OK to access 0 bytes at offset 0 in a dropped segment.
(invoke "init" (i64.const 0) (i32.const 0) (i32.const 0))

;; data.drop
(module
  (memory i64 1)
  (data "")
  (data (i64.const 0) "")

  (func (export "drop_passive") (data.drop 0))
  (func (export "init_passive")
    (memory.init 0 (i64.const 0) (i32.const 0) (i32.const 0)))

  (func (export "drop_active") (data.drop 1))
  (func (export "init_active")
    (memory.init 1 (i64.const 0) (i32.const 0) (i32.const 0)))
)

;; OK to drop the same segment multiple times or drop an active segment.
(invoke "init_passive")
(invoke "drop_passive")
(invoke "drop_passive")
(invoke "drop_active")
