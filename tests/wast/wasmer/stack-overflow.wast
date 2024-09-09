(module
      (type (;0;) (func))
      (func (;0;) (type 0)
        i32.const 0
        call_indirect (type 0))
      (table (;0;) 1 funcref)
      (export "stack-overflow" (func 0))
      (elem (;0;) (i32.const 0) 0))

(assert_exhaustion (invoke "stack-overflow") "call stack exhausted")
