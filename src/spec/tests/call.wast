(module
 (table 0 anyfunc)
 (memory 0)
;;  (func $for_2 (; 0 ;) (param $0 i32) (result i32)
;;   (i32.shl
;;    (get_local $0)
;;    (i32.const 1)
;;   )
;;  )
;;  (func (export "main") (; 1 ;) (result i32)
;;   (call $for_2
;;    (i32.const 2)
;;   )
;;  )

 (func (export "multiply") (; 1 ;) (result i32)
  (i32.shl
   (i32.const 3)
   (i32.const 2)
  )
 )

)

(assert_return (invoke "multiply") (i32.const 6))
