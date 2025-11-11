(module
  (import "test" "log" (func $log (param i32 i32)))
  (import "test" "continuation_new" (func $continuation_new (param i32) (result i32)))
  (import "test" "continuation_switch" (func $continuation_switch (param i32)))
  
  
  (memory (export "memory") 1)
  
  (data (i32.const 000) "[gr1] main  -> test1")
  (data (i32.const 100) "[gr1] test1 <- test2")
  (data (i32.const 200) "[gr2] test1 -> test2")
  (data (i32.const 300) "[main] main <- test1")

  (global $main (mut i32) (i32.const 0))
  (global $gr1 (mut i32) (i32.const 0))
  (global $gr2 (mut i32) (i32.const 0))
  
  (func (export "_main")
    i32.const 0  
    global.set $main
    (call $continuation_new (i32.const 0))
    global.set $gr1
    (call $continuation_new (i32.const 1))
    global.set $gr2

    ;; Switch to gr1
    global.get $gr1
    (call $continuation_switch)

    ;; Print [main] main <- test1
    i32.const 300  
    i32.const 20 
    call $log
  )
  (func $gr1 
    ;; Print [gr1] main  -> test1
    i32.const 0
    i32.const 20   
    call $log

    ;; Switch to gr2
    global.get $gr2
    call $continuation_switch

    ;; Print [gr1] test1 <- test2
    i32.const 100
    i32.const 20  
    call $log

    ;; Switch to gr2
    global.get $gr2
    call $continuation_switch

    ;; Print [gr1] test1 <- test2
    i32.const 100
    i32.const 20  
    call $log

    ;; Switch back to main
    global.get $main
    call $continuation_switch
    unreachable
  )
  (func $gr2 
    ;; Print [gr2] test1 -> test2
    i32.const 200  
    i32.const 20 
    call $log

    ;; Switch to gr1
    global.get $gr1
    call $continuation_switch

    ;; Print [gr2] test1 -> test2
    i32.const 200  
    i32.const 20 
    call $log

    ;; Switch to gr1
    global.get $gr1
    call $continuation_switch

    unreachable
  )
  (func (export "entrypoint") (param i32) 
    local.get 0
    i32.const 0
    i32.eq
    (if
      (then
    (call $gr1)
  )
  (else
    (call $gr2)
  ))
  unreachable
    
    
  )

)
