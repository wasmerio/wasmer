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

(assert_invalid
  (module (memory 1) (global i32 (i32.const 0)) (data (global.get 0) "a"))
  "unknown global"
)
(assert_invalid
  (module (memory 1) (global $g i32 (i32.const 0)) (data (global.get $g) "a"))
  "unknown global"
)


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
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 0 0)
    (data (i32.const 0) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 0 1)
    (data (i32.const 0) "a")
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (memory 0)
    (data (i32.const 1))
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (memory 0 1)
    (data (i32.const 1))
  )
  "out of bounds memory access"
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
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 1 2)
    (data (i32.const 0x1_0000) "a")
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const 0x1_0000) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 2)
    (data (i32.const 0x2_0000) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 2 3)
    (data (i32.const 0x2_0000) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 1)
    (data (i32.const -1) "a")
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const -1) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 2)
    (data (i32.const -100) "a")
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (import "spectest" "memory" (memory 1))
    (data (i32.const -100) "a")
  )
  "out of bounds memory access"
)

;; Data without memory

(assert_invalid
  (module
    (data (i32.const 0) "")
  )
  "unknown memory"
)

;; Data segment with memory index 1 (only memory 0 available)
(assert_invalid
  (module binary
    "\00asm" "\01\00\00\00"
    "\05\03\01"                             ;; memory section
    "\00\00"                                ;; memory 0
    "\0b\07\01"                             ;; data section
    "\02\01\41\00\0b"                       ;; active data segment 0 for memory 1
    "\00"                                   ;; empty vec(byte)
  )
  "unknown memory 1"
)

;; Data segment with memory index 0 (no memory section)
(assert_invalid
  (module binary
    "\00asm" "\01\00\00\00"
    "\0b\06\01"                             ;; data section
    "\00\41\00\0b"                          ;; active data segment 0 for memory 0
    "\00"                                   ;; empty vec(byte)
  )
  "unknown memory 0"
)

;; Data segment with memory index 1 (no memory section)
(assert_invalid
  (module binary
    "\00asm" "\01\00\00\00"
    "\0b\07\01"                             ;; data section
    "\02\01\41\00\0b"                       ;; active data segment 0 for memory 1
    "\00"                                   ;; empty vec(byte)
  )
  "unknown memory 1"
)

;; Data segment with memory index 1 and vec(byte) as above,
;; only memory 0 available.
(assert_invalid
  (module binary
    "\00asm" "\01\00\00\00"
    "\05\03\01"                             ;; memory section
    "\00\00"                                ;; memory 0
    "\0b\45\01"                             ;; data section
    "\02"                                   ;; active segment
    "\01"                                   ;; memory index
    "\41\00\0b"                             ;; offset constant expression
    "\3e"                                   ;; vec(byte) length
    "\00\01\02\03\04\05\06\07\08\09\0a\0b\0c\0d\0e\0f"
    "\10\11\12\13\14\15\16\17\18\19\1a\1b\1c\1d\1e\1f"
    "\20\21\22\23\24\25\26\27\28\29\2a\2b\2c\2d\2e\2f"
    "\30\31\32\33\34\35\36\37\38\39\3a\3b\3c\3d"
  )
  "unknown memory 1"
)

;; Data segment with memory index 1 and specially crafted vec(byte) after.
;; This is to detect incorrect validation where memory index is interpreted
;; as a flag followed by "\41" interpreted as the size of vec(byte)
;; with the expected number of bytes following.
(assert_invalid
  (module binary
    "\00asm" "\01\00\00\00"
    "\0b\45\01"                             ;; data section
    "\02"                                   ;; active segment
    "\01"                                   ;; memory index
    "\41\00\0b"                             ;; offset constant expression
    "\3e"                                   ;; vec(byte) length
    "\00\01\02\03\04\05\06\07\08\09\0a\0b\0c\0d\0e\0f"
    "\10\11\12\13\14\15\16\17\18\19\1a\1b\1c\1d\1e\1f"
    "\20\21\22\23\24\25\26\27\28\29\2a\2b\2c\2d\2e\2f"
    "\30\31\32\33\34\35\36\37\38\39\3a\3b\3c\3d"
  )
  "unknown memory 1"
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
    (data (ref.null func))
  )
  "type mismatch"
)

(assert_invalid
  (module 
    (memory 1)
    (data (offset (;empty instruction sequence;)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (memory 1)
    (data (offset (i32.const 0) (i32.const 0)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (global (import "test" "global-i32") i32)
    (memory 1)
    (data (offset (global.get 0) (global.get 0)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (global (import "test" "global-i32") i32)
    (memory 1)
    (data (offset (global.get 0) (i32.const 0)))
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

(assert_invalid
  (module
    (global $g (import "test" "g") (mut i32))
    (memory 1)
    (data (global.get $g))
  )
  "constant expression required"
)

(assert_invalid
   (module 
     (memory 1)
     (data (global.get 0))
   )
   "unknown global 0"
)

(assert_invalid
   (module
     (global (import "test" "global-i32") i32)
     (memory 1)
     (data (global.get 1))
   )
   "unknown global 1"
)

(assert_invalid
   (module 
     (global (import "test" "global-mut-i32") (mut i32))
     (memory 1)
     (data (global.get 0))
   )
   "constant expression required"
)
