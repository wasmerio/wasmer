;; https://github.com/wasmerio/wasmer/issues/2347
(module
    (type (;0;) (func (param f64) (result i32)))
    (func (;0;) (type 0) (param f64) (result i32)
        unreachable)
    (func (;1;) (type 0) (param f64) (result i32)
        i32.const -16579585
        f64.convert_i32_s
        f64.ceil
        f64.ceil
        local.get 0
        f64.copysign
    unreachable))
    