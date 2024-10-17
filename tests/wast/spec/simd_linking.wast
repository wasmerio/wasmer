(module
  (global (export "g-v128") v128 (v128.const i64x2 0 0))
  (global (export "mg-v128") (mut v128) (v128.const i64x2 0 0))
)
(register "Mv128")

(module
  ;; TODO: Reactivate once the fix for https://bugs.chromium.org/p/v8/issues/detail?id=13732
  ;; has made it to the downstream node.js that we use on CI.
  ;; (import "Mv128" "g-v128" (global v128))
  (import "Mv128" "mg-v128" (global (mut v128)))
)
