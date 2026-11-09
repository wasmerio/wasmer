;; Source of `sleep.asyncify.wasm`. Regenerate with:
;;
;;   wasm-tools parse sleep.wat -o sleep.wasm
;;   wasm-opt --asyncify --pass-arg=asyncify-imports@env.sleep sleep.wasm -o sleep.asyncify.wasm
;;   rm sleep.wasm
(module
  (import "env" "sleep" (func $sleep (param i32) (result i32)))
  (memory (export "memory") 1)
  ;; `before` is live across the call to `sleep`, so it must survive the
  ;; unwind and rewind.
  (func (export "run") (param $x i32) (result i32)
    (local $before i32)
    (local.set $before (i32.mul (local.get $x) (i32.const 10)))
    (i32.add (local.get $before) (call $sleep (local.get $x)))))
