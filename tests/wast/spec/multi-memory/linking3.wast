(module $Mm
  (memory $mem0 (export "mem0") 0 0)
  (memory $mem1 (export "mem1") 5 5)
  (memory $mem2 (export "mem2") 0 0)
  
  (data (memory 1) (i32.const 10) "\00\01\02\03\04\05\06\07\08\09")

  (func (export "load") (param $a i32) (result i32)
    (i32.load8_u $mem1 (local.get 0))
  )
)
(register "Mm" $Mm)

(assert_unlinkable
  (module
    (func $host (import "spectest" "print"))
    (memory (import "Mm" "mem1") 1)
    (table (import "Mm" "tab") 0 funcref)  ;; does not exist
    (data (i32.const 0) "abc")
  )
  "unknown import"
)
(assert_return (invoke $Mm "load" (i32.const 0)) (i32.const 0))

;; Unlike in v1 spec, active data segments written before an
;; out-of-bounds access persist after the instantiation failure.
(assert_trap
  (module
    ;; Note: the memory is 5 pages large by the time we get here.
    (memory (import "Mm" "mem1") 1)
    (data (i32.const 0) "abc")
    (data (i32.const 327670) "zzzzzzzzzzzzzzzzzz") ;; (partially) out of bounds
  )
  "out of bounds memory access"
)
(assert_return (invoke $Mm "load" (i32.const 0)) (i32.const 97))
(assert_return (invoke $Mm "load" (i32.const 327670)) (i32.const 0))

(assert_trap
  (module
    (memory (import "Mm" "mem1") 1)
    (data (i32.const 0) "abc")
    (table 0 funcref)
    (func)
    (elem (i32.const 0) 0)  ;; out of bounds
  )
  "out of bounds table access"
)
(assert_return (invoke $Mm "load" (i32.const 0)) (i32.const 97))

;; Store is modified if the start function traps.
(module $Ms
  (type $t (func (result i32)))
  (memory (export "memory") 1)
  (table (export "table") 1 funcref)
  (func (export "get memory[0]") (type $t)
    (i32.load8_u (i32.const 0))
  )
  (func (export "get table[0]") (type $t)
    (call_indirect (type $t) (i32.const 0))
  )
)
(register "Ms" $Ms)

(assert_trap
  (module
    (import "Ms" "memory" (memory 1))
    (import "Ms" "table" (table 1 funcref))
    (data (i32.const 0) "hello")
    (elem (i32.const 0) $f)
    (func $f (result i32)
      (i32.const 0xdead)
    )
    (func $main
      (unreachable)
    )
    (start $main)
  )
  "unreachable"
)

(assert_return (invoke $Ms "get memory[0]") (i32.const 104))  ;; 'h'
(assert_return (invoke $Ms "get table[0]") (i32.const 0xdead))
