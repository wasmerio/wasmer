;; Control exports are module-local; their presence does not establish cross-module replay.
(module
  (import "env" "callee" (func $callee))
  (global (export "__stack_pointer") (mut i32) (i32.const 1024))
  (func (export "asyncify_start_unwind") (param i32))
  (func (export "asyncify_stop_unwind"))
  (func (export "asyncify_start_rewind") (param i32))
  (func (export "asyncify_stop_rewind"))
  (func (export "entry") call $callee))
