(module $Mm
  (memory $mem0 (export "mem0") 0 0)
  (memory $mem1 (export "mem1") 1 5)
  (memory $mem2 (export "mem2") 0 0)
  
  (data (memory 1) (i32.const 10) "\00\01\02\03\04\05\06\07\08\09")

  (func (export "load") (param $a i32) (result i32)
    (i32.load8_u $mem1 (local.get 0))
  )
)
(register "Mm" $Mm)

(module $Nm
  (func $loadM (import "Mm" "load") (param i32) (result i32))
  (memory (import "Mm" "mem0") 0)

  (memory $m 1)
  (data (memory 1) (i32.const 10) "\f0\f1\f2\f3\f4\f5")

  (export "Mm.load" (func $loadM))
  (func (export "load") (param $a i32) (result i32)
    (i32.load8_u $m (local.get 0))
  )
)

(assert_return (invoke $Mm "load" (i32.const 12)) (i32.const 2))
(assert_return (invoke $Nm "Mm.load" (i32.const 12)) (i32.const 2))
(assert_return (invoke $Nm "load" (i32.const 12)) (i32.const 0xf2))

(module $Om
  (memory (import "Mm" "mem1") 1)
  (data (i32.const 5) "\a0\a1\a2\a3\a4\a5\a6\a7")

  (func (export "load") (param $a i32) (result i32)
    (i32.load8_u (local.get 0))
  )
)

(assert_return (invoke $Mm "load" (i32.const 12)) (i32.const 0xa7))
(assert_return (invoke $Nm "Mm.load" (i32.const 12)) (i32.const 0xa7))
(assert_return (invoke $Nm "load" (i32.const 12)) (i32.const 0xf2))
(assert_return (invoke $Om "load" (i32.const 12)) (i32.const 0xa7))

(module
  (memory (import "Mm" "mem1") 0)
  (data (i32.const 0xffff) "a")
)

(assert_trap
  (module
    (memory (import "Mm" "mem0") 0)
    (data (i32.const 0xffff) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory (import "Mm" "mem1") 0)
    (data (i32.const 0x10000) "a")
  )
  "out of bounds memory access"
)

