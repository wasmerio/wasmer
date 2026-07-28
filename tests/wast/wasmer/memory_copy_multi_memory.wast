;; Exercise memory.copy when the source and destination are different memories.

;; Copy from memory 0 to memory 1.
(module
  (memory $src 1)
  (memory $dst 1)

  (data (memory $src) (i32.const 8) "\11\22\33\44")
  (data (memory $dst) (i32.const 16) "\aa\bb\cc\dd")

  (func (export "copy") (result i32)
    (memory.copy $dst $src (i32.const 16) (i32.const 8) (i32.const 4))
    (i32.load $dst (i32.const 16))
  )

  (func (export "load-source") (result i32)
    (i32.load $src (i32.const 8))
  )
)

(assert_return (invoke "copy") (i32.const 0x44332211))
(assert_return (invoke "load-source") (i32.const 0x44332211))

;; Copy in the other direction, from memory 1 to memory 0.
(module
  (memory $dst 1)
  (memory $src 1)

  (data (memory $dst) (i32.const 24) "\aa\bb\cc\dd")
  (data (memory $src) (i32.const 32) "\55\66\77\88")

  (func (export "copy") (result i32)
    (memory.copy $dst $src (i32.const 24) (i32.const 32) (i32.const 4))
    (i32.load $dst (i32.const 24))
  )

  (func (export "load-source") (result i32)
    (i32.load $src (i32.const 32))
  )
)

(assert_return (invoke "copy") (i32.const 0x88776655))
(assert_return (invoke "load-source") (i32.const 0x88776655))
