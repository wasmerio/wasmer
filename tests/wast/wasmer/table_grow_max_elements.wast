(module
  (table $first 100000 10000000 funcref)
  (table $second 200000 10000000 funcref)

  (func (export "grow-first") (param $delta i32) (result i32)
    (table.grow $first (ref.null func) (local.get $delta)))

  (func (export "grow-second") (param $delta i32) (result i32)
    (table.grow $second (ref.null func) (local.get $delta)))

  (func (export "first-size") (result i32)
    (table.size $first))

  (func (export "second-size") (result i32)
    (table.size $second))
)

(assert_return (invoke "grow-first" (i32.const 400000)) (i32.const 100000))
(assert_return (invoke "grow-second" (i32.const 300000)) (i32.const 200000))
(assert_return (invoke "first-size") (i32.const 500000))
(assert_return (invoke "second-size") (i32.const 500000))

;; Growing either table beyond the remaining allocation room fails and leaves it unchanged.
(assert_return (invoke "grow-second" (i32.const 1)) (i32.const -1))
(assert_return (invoke "second-size") (i32.const 500000))
(assert_return (invoke "grow-first" (i32.const 0)) (i32.const 500000))
