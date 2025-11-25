(module
  (type (;0;) (func (param i32)))
  (type (;1;) (func))
  (import "wasi_snapshot_preview1" "proc_exit" (func (;0;) (type 0)))
  (func (;1;) (type 1)
    (local i32)
    (local.set 0
      (i32.const 0))
    (loop

      (drop
        (f64.min
          (f64.const 0x1.1ff8b184f99c5p+1020 (;=1.26388e+307;))
          (f64.const 0x1.1ff8b184f99c5p+1020 (;=1.26388e+307;))))

      (local.set 0
        (i32.add
          (local.get 0)
          (i32.const 1)))
      (br_if 0
        (i32.ne
          (local.get 0)
          (i32.const 0))))
    (call 0
      (i32.const 0))
    (unreachable))
  (export "_start" (func 1))
  (memory (;0;) 1)
  (export "memory" (memory 0)))
