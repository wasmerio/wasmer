(module
  (memory 1)

  (func (export "load-at-4gib")
    (local $base i32)
    (local.set $base (i32.const -2147483648))
    (drop (i64.load offset=2147483648 (local.get $base)))
  )

  (func (export "load-at-5gib")
    (local $base i32)
    (local.set $base (i32.const -1073741824))
    (drop (i64.load offset=2147483648 (local.get $base)))
  )

  (func (export "load-at-6gib")
    (local $base i32)
    (local.set $base (i32.const -2147483647))
    (drop (i64.load offset=4294967295 (local.get $base)))
  )

  (func (export "load-at-7gib")
    (local $base i32)
    (local.set $base (i32.const -1073741823))
    (drop (i64.load offset=4294967295 (local.get $base)))
  )

  (func (export "load-at-8gib")
    (local $base i32)
    (local.set $base (i32.const -1))
    (drop (i64.load offset=4294967295 (local.get $base)))
  )
)

(assert_trap (invoke "load-at-4gib") "out of bounds memory access")
(assert_trap (invoke "load-at-5gib") "out of bounds memory access")
(assert_trap (invoke "load-at-6gib") "out of bounds memory access")
(assert_trap (invoke "load-at-7gib") "out of bounds memory access")
(assert_trap (invoke "load-at-8gib") "out of bounds memory access")
