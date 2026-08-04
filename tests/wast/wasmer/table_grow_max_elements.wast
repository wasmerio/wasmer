
(module
  (table $table 0 10000000 funcref)

  (func (export "grow") (param $delta i32) (result i32)
    (table.grow $table (ref.null func) (local.get $delta)))

  (func (export "size") (result i32)
    (table.size $table))
)

;; Growing to TABLE_MAX_ELEMENTS succeeds.
(assert_return (invoke "grow" (i32.const 1000000)) (i32.const 0))
(assert_return (invoke "size") (i32.const 1000000))

;; Growing beyond TABLE_MAX_ELEMENTS fails and leaves the table unchanged.
(assert_return (invoke "grow" (i32.const 1)) (i32.const -1))
(assert_return (invoke "size") (i32.const 1000000))
