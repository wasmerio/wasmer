
(module (memory 0 0))
(module (memory 0 1))
(module (memory 1 256))
(module (memory 0 65536))

(assert_invalid (module (memory 0) (memory 0)) "multiple memories")
(assert_invalid (module (memory (import "spectest" "memory") 0) (memory 0)) "multiple memories")

(module (memory (data)) (func (export "memsize") (result i32) (memory.size)))
(assert_return (invoke "memsize") (i32.const 0))
(module (memory (data "")) (func (export "memsize") (result i32) (memory.size)))
(assert_return (invoke "memsize") (i32.const 0))
(module (memory (data "x")) (func (export "memsize") (result i32) (memory.size)))
(assert_return (invoke "memsize") (i32.const 1))

(assert_invalid (module (data (i32.const 0))) "unknown memory")
(assert_invalid (module (data (i32.const 0) "")) "unknown memory")
(assert_invalid (module (data (i32.const 0) "x")) "unknown memory")

(assert_invalid
  (module (func (drop (f32.load (i32.const 0)))))
  "unknown memory"
)
(assert_invalid
  (module (func (f32.store (f32.const 0) (i32.const 0))))
  "unknown memory"
)
(assert_invalid
  (module (func (drop (i32.load8_s (i32.const 0)))))
  "unknown memory"
)
(assert_invalid
  (module (func (i32.store8 (i32.const 0) (i32.const 0))))
  "unknown memory"
)
(assert_invalid
  (module (func (drop (memory.size))))
  "unknown memory"
)
(assert_invalid
  (module (func (drop (memory.grow (i32.const 0)))))
  "unknown memory"
)


(assert_invalid
  (module (memory 1 0))
  "size minimum must not be greater than maximum"
)
(assert_invalid
  (module (memory 65537))
  "memory size must be at most 65536 pages (4GiB)"
)
(assert_invalid
  (module (memory 2147483648))
  "memory size must be at most 65536 pages (4GiB)"
)
(assert_invalid
  (module (memory 4294967295))
  "memory size must be at most 65536 pages (4GiB)"
)
(assert_invalid
  (module (memory 0 65537))
  "memory size must be at most 65536 pages (4GiB)"
)
(assert_invalid
  (module (memory 0 2147483648))
  "memory size must be at most 65536 pages (4GiB)"
)
(assert_invalid
  (module (memory 0 4294967295))
  "memory size must be at most 65536 pages (4GiB)"
)
