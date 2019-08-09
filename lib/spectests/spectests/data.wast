;; Test the data section

;; Syntax

(module
  (memory $m 1)
  (data (i32.const 0))
  (data (i32.const 1) "a" "" "bcd")
  (data (offset (i32.const 0)))
  (data (offset (i32.const 0)) "" "a" "bc" "")
  (data 0 (i32.const 0))
  (data 0x0 (i32.const 1) "a" "" "bcd")
  (data 0x000 (offset (i32.const 0)))
  (data 0 (offset (i32.const 0)) "" "a" "bc" "")
  (data $m (i32.const 0))
  (data $m (i32.const 1) "a" "" "bcd")
  (data $m (offset (i32.const 0)))
  (data $m (offset (i32.const 0)) "" "a" "bc" "")
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

(assert_unlinkable
  (module
    (memory 0)
    (data (i32.const 0) "a")
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 0 0)
    (data (i32.const 0) "a")
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 0 1)
    (data (i32.const 0) "a")
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 0)
    (data (i32.const 1))
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 0 1)
    (data (i32.const 1))
  )
  "data segment does not fit"
)

;; This seems to cause a time-out on Travis.
(;assert_unlinkable
  (module
    (memory 0x10000)
    (data (i32.const 0xffffffff) "ab")
  )
  ""  ;; either out of memory or segment does not fit
;)

(assert_unlinkable
  (module
    (global (import "spectest" "global_i32") i32)
    (memory 0)
    (data (global.get 0) "a")
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 1 2)
    (data (i32.const 0x1_0000) "a")
  )
  "data segment does not fit"
)
(assert_unlinkable
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const 0x1_0000) "a")
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 2)
    (data (i32.const 0x2_0000) "a")
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 2 3)
    (data (i32.const 0x2_0000) "a")
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 1)
    (data (i32.const -1) "a")
  )
  "data segment does not fit"
)
(assert_unlinkable
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const -1) "a")
  )
  "data segment does not fit"
)

(assert_unlinkable
  (module
    (memory 2)
    (data (i32.const -100) "a")
  )
  "data segment does not fit"
)
(assert_unlinkable
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const -100) "a")
  )
  "data segment does not fit"
)

;; Data without memory

(assert_invalid
  (module
    (data (i32.const 0) "")
  )
  "unknown memory 0"
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
