(module
  (memory 0 0)
  (memory $m 1 1)
  (memory 0 0)
  (func (export "init") (param $i i32) (param $x f64)
    (f64.store $m (local.get $i) (local.get $x)))

  (func (export "run") (param $n i32) (param $z f64)
    (local $i i32)
    (block $exit
      (loop $cont
        (f64.store $m
          (local.get $i)
          (f64.div (f64.load $m (local.get $i)) (local.get $z))
        )
        (local.set $i (i32.add (local.get $i) (i32.const 8)))
        (br_if $cont (i32.lt_u (local.get $i) (local.get $n)))
      )
    )
  )

  (func (export "check") (param $i i32) (result f64) (f64.load $m (local.get $i)))
)

(invoke "init" (i32.const  0) (f64.const 15.1))
(invoke "init" (i32.const  8) (f64.const 15.2))
(invoke "init" (i32.const 16) (f64.const 15.3))
(invoke "init" (i32.const 24) (f64.const 15.4))
(assert_return (invoke "check" (i32.const  0)) (f64.const 15.1))
(assert_return (invoke "check" (i32.const  8)) (f64.const 15.2))
(assert_return (invoke "check" (i32.const 16)) (f64.const 15.3))
(assert_return (invoke "check" (i32.const 24)) (f64.const 15.4))
(invoke "run" (i32.const 32) (f64.const 3.0))
(assert_return (invoke "check" (i32.const 0)) (f64.const 0x1.4222222222222p+2))
(assert_return (invoke "check" (i32.const 8)) (f64.const 0x1.4444444444444p+2))
(assert_return (invoke "check" (i32.const 16)) (f64.const 0x1.4666666666667p+2))
(assert_return (invoke "check" (i32.const 24)) (f64.const 0x1.4888888888889p+2))

