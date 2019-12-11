(module
  (import "wasi_unstable" "proc_exit"      (func $proc_exit (param i32)))
  (export "_start" (func $_start))

  (memory 10)
  (export "memory" (memory 0))

  (func $_start
    (call $proc_exit (i32.const 7))
  )
)
