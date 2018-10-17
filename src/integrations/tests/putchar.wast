(module
 (type $FUNCSIG$ii (func (param i32) (result i32)))
 (import "env" "putchar" (func $putchar (param i32) (result i32)))
 (table 0 anyfunc)
 (memory $0 1)
 (export "memory" (memory $0))
 (export "main" (func $main))
 (func $main (; 1 ;) (result i32)
  (drop
   (call $putchar
    (i32.const 97)
   )
  )
  (i32.const 0)
 )
)
