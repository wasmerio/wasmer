(module
    (func $add (param $x f64) (param $y f64) (result f64) (f64.add (local.get $x) (local.get $y)))
    (func $sub (param $x f64) (param $y f64) (result f64) (f64.sub (local.get $x) (local.get $y)))
    (func $mul (param $x f64) (param $y f64) (result f64) (f64.mul (local.get $x) (local.get $y)))
    (func $div (param $x f64) (param $y f64) (result f64) (f64.div (local.get $x) (local.get $y)))
    (func $sqrt (param $x f64) (result f64) (f64.sqrt (local.get $x)))
    (func $min (param $x f64) (param $y f64) (result f64) (f64.min (local.get $x) (local.get $y)))
    (func $max (param $x f64) (param $y f64) (result f64) (f64.max (local.get $x) (local.get $y)))
    (func (export "ceil") (param $x f64) (result f64) (f64.ceil (local.get $x)))
    (func (export "floor") (param $x f64) (result f64) (f64.floor (local.get $x)))
    (func (export "trunc") (param $x f64) (result f64) (f64.trunc (local.get $x)))
    (func (export "nearest") (param $x f64) (result f64) (f64.nearest (local.get $x)))
    (func $main (export "main") (result f64)
        ;; (call $add (f64.promote_f32 (f32.const 0x1p+0)) (f64.const 0x2p+0)) ;; 3.0
        ;; (call $sub (f64.promote_f32 (f32.const 0x1p+0)) (f64.const 0x2p+0)) ;; -1.0
        ;; (call $mul (f64.promote_f32 (f32.const 0x2p+0)) (f64.const 0x3p+0)) ;; 6.0
        ;; (call $div (f64.promote_f32 (f32.const 0xap+0)) (f64.const 0x2p+0)) ;; 5.0
        ;; (call $sqrt (f64.const 0x10p+0)) ;; 4.0
        ;; (call $min (f64.promote_f32 (f32.const 0x1p+0)) (f64.const 0x2p+0)) ;; 1.0
        ;; (call $max (f64.promote_f32 (f32.const 0x1p+0)) (f64.const 0x2p+0)) ;; 2.0
    )
)
