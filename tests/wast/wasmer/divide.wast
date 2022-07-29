(module
  (func (export "div_s") (param $x i64) (param $y i64) (result i64) (i64.div_s (local.get $x) (local.get $y)))
)
(assert_trap (invoke "div_s" (i64.const 1) (i64.const 0)) "integer divide by zero")
