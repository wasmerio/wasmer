;; Test the data section

;; Syntax

(module
  (memory $m 1)
  (data (i32.const 0))
  (data (i32.const 1) "a" "" "bcd")
  (data (offset (i32.const 0)))
  (data (offset (i32.const 0)) "" "a" "bc" "")
  (data (memory 0) (i32.const 0))
  (data (memory 0x0) (i32.const 1) "a" "" "bcd")
  (data (memory 0x000) (offset (i32.const 0)))
  (data (memory 0) (offset (i32.const 0)) "" "a" "bc" "")
  (data (memory $m) (i32.const 0))
  (data (memory $m) (i32.const 1) "a" "" "bcd")
  (data (memory $m) (offset (i32.const 0)))
  (data (memory $m) (offset (i32.const 0)) "" "a" "bc" "")
  (data $d1 (i32.const 0))
  (data $d2 (i32.const 1) "a" "" "bcd")
  (data $d3 (offset (i32.const 0)))
  (data $d4 (offset (i32.const 0)) "" "a" "bc" "")
  (data $d5 (memory 0) (i32.const 0))
  (data $d6 (memory 0x0) (i32.const 1) "a" "" "bcd")
  (data $d7 (memory 0x000) (offset (i32.const 0)))
  (data $d8 (memory 0) (offset (i32.const 0)) "" "a" "bc" "")
  (data $d9 (memory $m) (i32.const 0))
  (data $d10 (memory $m) (i32.const 1) "a" "" "bcd")
  (data $d11 (memory $m) (offset (i32.const 0)))
  (data $d12 (memory $m) (offset (i32.const 0)) "" "a" "bc" "")
)

;; Basic use

(module
  (memory 1)
  (data (i32.const 0) "a")
)
(module
  (import "spectest" "memory" (memory 1))
  (data (i32.const 0) "a")
)

(module
  (memory 1)
  (data (i32.const 0) "a")
  (data (i32.const 3) "b")
  (data (i32.const 100) "cde")
  (data (i32.const 5) "x")
  (data (i32.const 3) "c")
)
(module
  (import "spectest" "memory" (memory 1))
  (data (i32.const 0) "a")
  (data (i32.const 1) "b")
  (data (i32.const 2) "cde")
  (data (i32.const 3) "f")
  (data (i32.const 2) "g")
  (data (i32.const 1) "h")
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

;; Use of internal globals in constant expressions is not allowed in MVP.
;; (module (memory 1) (data (global.get 0) "a") (global i32 (i32.const 0)))
;; (module (memory 1) (data (global.get $g) "a") (global $g i32 (i32.const 0)))

;; Corner cases

(module
  (memory 1)
  (data (i32.const 0) "a")
  (data (i32.const 0xffff) "b")
)
(module
  (import "spectest" "memory" (memory 1))
  (data (i32.const 0) "a")
  (data (i32.const 0xffff) "b")
)

(module
  (memory 2)
  (data (i32.const 0x1_ffff) "a")
)

(module
  (memory 0)
  (data (i32.const 0))
)
(module
  (import "spectest" "memory" (memory 0))
  (data (i32.const 0))
)

(module
  (memory 0 0)
  (data (i32.const 0))
)

(module
  (memory 1)
  (data (i32.const 0x1_0000) "")
)

(module
  (memory 0)
  (data (i32.const 0) "" "")
)
(module
  (import "spectest" "memory" (memory 0))
  (data (i32.const 0) "" "")
)

(module
  (memory 0 0)
  (data (i32.const 0) "" "")
)

(module
  (import "spectest" "memory" (memory 0))
  (data (i32.const 0) "a")
)

(module
  (import "spectest" "memory" (memory 0 3))
  (data (i32.const 0) "a")
)

(module
  (global (import "spectest" "global_i32") i32)
  (import "spectest" "memory" (memory 0))
  (data (global.get 0) "a")
)

(module
  (global (import "spectest" "global_i32") i32)
  (import "spectest" "memory" (memory 0 3))
  (data (global.get 0) "a")
)

(module
  (import "spectest" "memory" (memory 0))
  (data (i32.const 1) "a")
)

(module
  (import "spectest" "memory" (memory 0 3))
  (data (i32.const 1) "a")
)

;; Invalid bounds for data

(assert_trap
  (module
    (memory 0)
    (data (i32.const 0) "a")
  )
  "out of bounds"
)

(assert_trap
  (module
    (memory 0 0)
    (data (i32.const 0) "a")
  )
  "out of bounds"
)

(assert_trap
  (module
    (memory 0 1)
    (data (i32.const 0) "a")
  )
  "out of bounds"
)
(assert_trap
  (module
    (memory 0)
    (data (i32.const 1))
  )
  "out of bounds"
)
(assert_trap
  (module
    (memory 0 1)
    (data (i32.const 1))
  )
  "out of bounds"
)

;; This seems to cause a time-out on Travis.
(;assert_unlinkable
  (module
    (memory 0x10000)
    (data (i32.const 0xffffffff) "ab")
  )
  ""  ;; either out of memory or out of bounds
;)

(assert_trap
  (module
    (global (import "spectest" "global_i32") i32)
    (memory 0)
    (data (global.get 0) "a")
  )
  "out of bounds"
)

(assert_trap
  (module
    (memory 1 2)
    (data (i32.const 0x1_0000) "a")
  )
  "out of bounds"
)
(assert_trap
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const 0x1_0000) "a")
  )
  "out of bounds"
)

(assert_trap
  (module
    (memory 2)
    (data (i32.const 0x2_0000) "a")
  )
  "out of bounds"
)

(assert_trap
  (module
    (memory 2 3)
    (data (i32.const 0x2_0000) "a")
  )
  "out of bounds"
)

(assert_trap
  (module
    (memory 1)
    (data (i32.const -1) "a")
  )
  "out of bounds"
)
(assert_trap
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const -1) "a")
  )
  "out of bounds"
)

(assert_trap
  (module
    (memory 2)
    (data (i32.const -100) "a")
  )
  "out of bounds"
)
(assert_trap
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const -100) "a")
  )
  "out of bounds"
)

;; Data without memory

(assert_invalid
  (module
    (data (i32.const 0) "")
  )
  "unknown memory"
)

;; Invalid offsets

(assert_invalid
  (module
    (memory 1)
    (data (i64.const 0))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (memory 1)
    (data (i32.ctz (i32.const 0)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (memory 1)
    (data (nop))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (memory 1)
    (data (offset (nop) (i32.const 0)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (memory 1)
    (data (offset (i32.const 0) (nop)))
  )
  "constant expression required"
)

;; Use of internal globals in constant expressions is not allowed in MVP.
;; (assert_invalid
;;   (module (memory 1) (data (global.get $g)) (global $g (mut i32) (i32.const 0)))
;;   "constant expression required"
;; )
