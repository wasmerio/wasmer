(module
  (import "env" "memory" (memory 1 4 shared))
  (func (export "tick")
    (i32.store (i32.const 256)
      (i32.add (i32.load (i32.const 256)) (i32.const 1)))))
