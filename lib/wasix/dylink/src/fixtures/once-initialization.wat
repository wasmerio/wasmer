;; Exact wasm32 shared-PIC once template from local lld/wasm/Writer.cpp.
(module
  (import "env" "__memory_base" (global $base i32))
  (memory 1 1 shared)
  (data $data "test")
  (func $init (local i32 i32)
    global.get $base i32.const 8 i32.add local.set 0
    block block block
      local.get 0 i32.const 0 i32.const 1 i32.atomic.rmw.cmpxchg
      br_table 0 1 2
    end
      i32.const 0 global.get $base i32.add
      i32.const 0 i32.const 4 memory.init $data
      local.get 0 i32.const 2 i32.atomic.store
      local.get 0 i32.const -1 memory.atomic.notify drop
      br 1
    end
      local.get 0 i32.const 1 i64.const -1 memory.atomic.wait32 drop
    end
    data.drop $data)
  (start $init))
