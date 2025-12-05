;; This example crashed in early versions of our async API because switching from the first context to another one worked only once.
(module
  (import "test" "log" (func $log (param i32 i32)))
  (import "test" "greenthread_new" (func $greenthread_new (param i32) (result i32)))
  (import "test" "greenthread_switch" (func $greenthread_switch (param i32)))
  
  (memory (export "memory") 1)
  
  (data (i32.const 0) "[main] switching to side")
  (data (i32.const 100) "[side] switching to main")
  (data (i32.const 200) "[main] returned")

  (global $main (mut i32) (i32.const 0))
  (global $side (mut i32) (i32.const 0))
  
  (func (export "_main")
    ;; Setup thread ids
    i32.const 0  
    global.set $main
    (call $greenthread_new (i32.const 0))
    global.set $side

    ;; Print [main] switching to side
    i32.const 0
    i32.const 30   
    call $log

    ;; Switch to side
    global.get $side
    (call $greenthread_switch)

    ;; Print [main] switching to side
    i32.const 0
    i32.const 30
    call $log

    ;; Switch to side
    global.get $side
    (call $greenthread_switch)

    ;; Print [main] returned
    i32.const 200
    i32.const 30
    call $log
  )
  (func $side 
    ;; Print [side] switching to main
    i32.const 100
    i32.const 30   
    call $log

    ;; Switch to main
    global.get $main
    call $greenthread_switch

    ;; Print [side] switching to main
    i32.const 100
    i32.const 30   
    call $log

    ;; Switch to main
    global.get $main
    call $greenthread_switch

    unreachable
  )
  (func (export "entrypoint") (param i32) 
    local.get 0
    i32.const 0
    i32.eq
    call $side
    unreachable
  )

)
