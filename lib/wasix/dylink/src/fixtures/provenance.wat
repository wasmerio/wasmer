;; A DSO can supply its own token explicitly, but a shared main-owned C wrapper cannot infer
;; the caller's token from its own __dso_handle. This fixture proves only retained metadata.
(module
  (import "env" "__memory_base" (global $base i32))
  (global (export "__dso_handle") i32 (global.get $base))
  (func (export "caller_token") (result i32) global.get $base))
