;; Negative trust fixture: linker-like names and a valid module do not authenticate helper
;; bodies. Parsing/planning must never call this start function or its user-code import.
(module
  (import "fixture" "user_code" (func $user))
  (func $init (export "__wasm_init_memory") call $user)
  (func (export "__wasm_init_tls") (param i32) call $user)
  (start $init))
