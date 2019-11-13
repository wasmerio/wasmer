(module
  (memory 1)
  (data (i32.const 0) "abcdefghijklmnopqrstuvwxyz")

  (func $f_8u_good1 (param $i i32) (result i64)
    (i64.load8_u offset=0 (local.get $i))                   ;; 97 'a'
  )
  (func $main (export "main") (result i64)
    (call $f_8u_good1 (i32.const 0))
  )
)