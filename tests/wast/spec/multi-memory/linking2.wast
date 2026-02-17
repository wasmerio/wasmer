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

(module $Pm
  (memory (import "Mm" "mem1") 1 8)

  (func (export "grow") (param $a i32) (result i32)
    (memory.grow (local.get 0))
  )
)

(assert_return (invoke $Pm "grow" (i32.const 0)) (i32.const 1))
(assert_return (invoke $Pm "grow" (i32.const 2)) (i32.const 1))
(assert_return (invoke $Pm "grow" (i32.const 0)) (i32.const 3))
(assert_return (invoke $Pm "grow" (i32.const 1)) (i32.const 3))
(assert_return (invoke $Pm "grow" (i32.const 1)) (i32.const 4))
(assert_return (invoke $Pm "grow" (i32.const 0)) (i32.const 5))
(assert_return (invoke $Pm "grow" (i32.const 1)) (i32.const -1))
(assert_return (invoke $Pm "grow" (i32.const 0)) (i32.const 5))

