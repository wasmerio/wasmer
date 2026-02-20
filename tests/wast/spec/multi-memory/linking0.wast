(module $Mt
  (type (func (result i32)))
  (type (func))

  (table (export "tab") 10 funcref)
  (elem (i32.const 2) $g $g $g $g)
  (func $g (result i32) (i32.const 4))
  (func (export "h") (result i32) (i32.const -4))

  (func (export "call") (param i32) (result i32)
    (call_indirect (type 0) (local.get 0))
  )
)
(register "Mt" $Mt)

(assert_unlinkable
  (module
    (table (import "Mt" "tab") 10 funcref)
    (memory (import "spectest" "memory") 1)
    (memory (import "Mt" "mem") 1)  ;; does not exist
    (func $f (result i32) (i32.const 0))
    (elem (i32.const 7) $f)
    (elem (i32.const 9) $f)
  )
  "unknown import"
)
(assert_trap (invoke $Mt "call" (i32.const 7)) "uninitialized element")


(assert_trap
  (module
    (table (import "Mt" "tab") 10 funcref)
    (func $f (result i32) (i32.const 0))
    (elem (i32.const 7) $f)
    (memory 0)
    (memory $m 1)
    (memory 0)
    (data $m (i32.const 0x10000) "d")  ;; out of bounds
  )
  "out of bounds memory access"
)
(assert_return (invoke $Mt "call" (i32.const 7)) (i32.const 0))
