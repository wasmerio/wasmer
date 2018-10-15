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

 (func $multiply (; 1 ;) (param i32 i32)  (result i32)
  (i32.mul
   (get_local 0)
   (get_local 1)
  )
 )

 (func (export "multiply_by_3") (; 1 ;) (param $0 i32) (result i32)
  (call $multiply
   (get_local $0)
   (i32.const 3)
  )
 )

 (func (export "multiply_by_3_raw") (; 1 ;) (param $0 i32) (result i32)
  (i32.mul
   (get_local $0)
   (i32.const 3)
  )
 )

)

(assert_return (invoke "multiply_by_3_raw" (i32.const 2)) (i32.const 6))
