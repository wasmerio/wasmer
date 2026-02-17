;; Test the data section

;; Syntax

(module
  (memory $mem0 1)
  (memory $mem1 1)
  (memory $mem2 1)
  
  (data (i32.const 0))
  (data (i32.const 1) "a" "" "bcd")
  (data (offset (i32.const 0)))
  (data (offset (i32.const 0)) "" "a" "bc" "")
  (data (memory 0) (i32.const 0))
  (data (memory 0x0) (i32.const 1) "a" "" "bcd")
  (data (memory 0x000) (offset (i32.const 0)))
  (data (memory 0) (offset (i32.const 0)) "" "a" "bc" "")
  (data (memory $mem0) (i32.const 0))
  (data (memory $mem1) (i32.const 1) "a" "" "bcd")
  (data (memory $mem2) (offset (i32.const 0)))
  (data (memory $mem0) (offset (i32.const 0)) "" "a" "bc" "")

  (data $d1 (i32.const 0))
  (data $d2 (i32.const 1) "a" "" "bcd")
  (data $d3 (offset (i32.const 0)))
  (data $d4 (offset (i32.const 0)) "" "a" "bc" "")
  (data $d5 (memory 0) (i32.const 0))
  (data $d6 (memory 0x0) (i32.const 1) "a" "" "bcd")
  (data $d7 (memory 0x000) (offset (i32.const 0)))
  (data $d8 (memory 0) (offset (i32.const 0)) "" "a" "bc" "")
  (data $d9 (memory $mem0) (i32.const 0))
  (data $d10 (memory $mem1) (i32.const 1) "a" "" "bcd")
  (data $d11 (memory $mem2) (offset (i32.const 0)))
  (data $d12 (memory $mem0) (offset (i32.const 0)) "" "a" "bc" "")
)

;; Basic use

(module
  (memory 1)
  (data (i32.const 0) "a")
)
(module
  (import "spectest" "memory" (memory 1))
  (import "spectest" "memory" (memory 1))
  (import "spectest" "memory" (memory 1))
  (data (memory 0) (i32.const 0) "a")
  (data (memory 1) (i32.const 0) "a")
  (data (memory 2) (i32.const 0) "a")
)

(module
  (global (import "spectest" "global_i32") i32)
  (memory 1)
  (data (global.get 0) "a")
)
(module
  (global (import "spectest" "global_i32") i32)
  (import "spectest" "memory" (memory 1))
  (data (global.get 0) "a")
)

(module
  (global $g (import "spectest" "global_i32") i32)
  (memory 1)
  (data (global.get $g) "a")
)
(module
  (global $g (import "spectest" "global_i32") i32)
  (import "spectest" "memory" (memory 1))
  (data (global.get $g) "a")
)

