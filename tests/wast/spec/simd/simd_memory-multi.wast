;; From wasmtime misc_testsuite/multi-memory/simple.wast

;; Test syntax for load/store_lane immediates

(module
  (memory 1)
  (memory $m 1)

  (func
    (local $v v128)

    (drop (v128.load8_lane 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane 1 offset=0 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane 1 offset=0 align=1 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane 1 align=1 1 (i32.const 0) (local.get $v)))

    (drop (v128.load8_lane $m 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane $m offset=0 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane $m offset=0 align=1 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane $m align=1 1 (i32.const 0) (local.get $v)))

    (drop (v128.load8_lane 1 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane 1 offset=0 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane 1 offset=0 align=1 1 (i32.const 0) (local.get $v)))
    (drop (v128.load8_lane 1 align=1 1 (i32.const 0) (local.get $v)))

    (v128.store8_lane 1 (i32.const 0) (local.get $v))
    (v128.store8_lane offset=0 1 (i32.const 0) (local.get $v))
    (v128.store8_lane offset=0 align=1 1 (i32.const 0) (local.get $v))
    (v128.store8_lane align=1 1 (i32.const 0) (local.get $v))

    (v128.store8_lane $m 1 (i32.const 0) (local.get $v))
    (v128.store8_lane $m offset=0 1 (i32.const 0) (local.get $v))
    (v128.store8_lane $m offset=0 align=1 1 (i32.const 0) (local.get $v))
    (v128.store8_lane $m align=1 1 (i32.const 0) (local.get $v))

    (v128.store8_lane 1 1 (i32.const 0) (local.get $v))
    (v128.store8_lane 1 offset=0 1 (i32.const 0) (local.get $v))
    (v128.store8_lane 1 offset=0 align=1 1 (i32.const 0) (local.get $v))
    (v128.store8_lane 1 align=1 1 (i32.const 0) (local.get $v))
  )
)
