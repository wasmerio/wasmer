(module
  (func $sret
    (result i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
            i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
            i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
            i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 

      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 

      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 

      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
      i64.const 0 i64.const 0 i64.const 0 i64.const 0 
    )

  (func (export "long-loop")
    (local i32)
    i32.const 1000000
    local.set 0
    (loop
      call $sret
      drop drop drop drop drop drop drop drop drop drop drop drop drop drop drop drop
      drop drop drop drop drop drop drop drop drop drop drop drop drop drop drop drop
      drop drop drop drop drop drop drop drop drop drop drop drop drop drop drop drop
      drop drop drop drop drop drop drop drop drop drop drop drop drop drop drop drop
      local.get 0
      i32.const -1
      i32.add
      local.tee 0
      br_if 0
    )
  )
)

(assert_return (invoke "long-loop"))
