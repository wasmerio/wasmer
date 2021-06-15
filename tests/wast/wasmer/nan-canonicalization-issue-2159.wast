;; https://github.com/wasmerio/wasmer/issues/2159
(module
  (func (export "_start") (result f64)
    f64.const 0x0p+0 (;=0;)
    f64.const 0x0p+0 (;=0;)
    f64.const 0x0p+0 (;=0;)
    f64.div
    f64.copysign
  )
)