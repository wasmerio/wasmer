(module
 (type $FUNCSIG$ii (func (param i32) (result i32)))
 (import "env" "puts" (func $puts (param i32) (result i32)))
 (import "env" "_emscripten_memcpy_big" (func $_emscripten_memcpy_big (param i32 i32 i32) (result i32)))
 (table 0 anyfunc)
 (memory $0 1)
 (data (i32.const 16) "hello, world!\00")
 (export "memory" (memory $0))
 (export "main" (func $main))
 (func $main (; 1 ;) (result i32)
  (drop
   (call $puts
    (i32.const 16)
   )
  )
  (i32.const 0)
 )
)
