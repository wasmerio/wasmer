;; Invalid bounds for data

(assert_trap
  (module
    (memory 1)
    (memory 0)
    (memory 2)
    (data (memory 1) (i32.const 0) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 1 1)
    (memory 1 1)
    (memory 0 0)
    (data (memory 2) (i32.const 0) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 1 1)
    (memory 0 1)
    (memory 1 1)
    (data (memory 1) (i32.const 0) "a")
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (memory 1)
    (memory 1)
    (memory 0)
    (data (memory 2) (i32.const 1))
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (memory 1 1)
    (memory 1 1)
    (memory 0 1)
    (data (memory 2) (i32.const 1))
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
    (memory 3)
    (memory 0)
    (memory 3)
    (data (memory 1) (global.get 0) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 2 2)
    (memory 1 2)
    (memory 2 2)
    (data (memory 1) (i32.const 0x1_0000) "a")
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
    (memory 3)
    (memory 3)
    (memory 2)
    (data (memory 2) (i32.const 0x2_0000) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 3 3)
    (memory 2 3)
    (memory 3 3)
    (data (memory 1) (i32.const 0x2_0000) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 0)
    (memory 0)
    (memory 1)
    (data (memory 2) (i32.const -1) "a")
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (import "spectest" "memory" (memory 1))
    (import "spectest" "memory" (memory 1))
    (import "spectest" "memory" (memory 1))
    (data (memory 2) (i32.const -1) "a")
  )
  "out of bounds memory access"
)

(assert_trap
  (module
    (memory 2)
    (memory 2)
    (memory 2)
    (data (memory 2) (i32.const -100) "a")
  )
  "out of bounds memory access"
)
(assert_trap
  (module
    (import "spectest" "memory" (memory 1))
    (import "spectest" "memory" (memory 1))
    (import "spectest" "memory" (memory 1))
    (import "spectest" "memory" (memory 1))
    (data (memory 3) (i32.const -100) "a")
  )
  "out of bounds memory access"
)

