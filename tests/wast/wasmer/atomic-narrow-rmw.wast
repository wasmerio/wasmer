(module
  (memory 1 1 shared)

  ;; Each RMW uses a distinct zero-initialized address. The operands exceed the
  ;; range of the accessed type so stale upper bits in the result are observable.

  (func (export "i32.atomic.rmw8.add_u") (result i32)
    i32.const 0
    i32.const 300
    i32.atomic.rmw8.add_u)
  (func (export "i32.atomic.rmw16.add_u") (result i32)
    i32.const 8
    i32.const 70000
    i32.atomic.rmw16.add_u)
  (func (export "i64.atomic.rmw8.add_u") (result i64)
    i32.const 16
    i64.const 300
    i64.atomic.rmw8.add_u)
  (func (export "i64.atomic.rmw16.add_u") (result i64)
    i32.const 24
    i64.const 70000
    i64.atomic.rmw16.add_u)
  (func (export "i64.atomic.rmw32.add_u") (result i64)
    i32.const 32
    i64.const 4294967297
    i64.atomic.rmw32.add_u)

  (func (export "i32.atomic.rmw8.sub_u") (result i32)
    i32.const 40
    i32.const 300
    i32.atomic.rmw8.sub_u)
  (func (export "i32.atomic.rmw16.sub_u") (result i32)
    i32.const 48
    i32.const 70000
    i32.atomic.rmw16.sub_u)
  (func (export "i64.atomic.rmw8.sub_u") (result i64)
    i32.const 56
    i64.const 300
    i64.atomic.rmw8.sub_u)
  (func (export "i64.atomic.rmw16.sub_u") (result i64)
    i32.const 64
    i64.const 70000
    i64.atomic.rmw16.sub_u)
  (func (export "i64.atomic.rmw32.sub_u") (result i64)
    i32.const 72
    i64.const 4294967297
    i64.atomic.rmw32.sub_u)

  (func (export "i32.atomic.rmw8.and_u") (result i32)
    i32.const 80
    i32.const 300
    i32.atomic.rmw8.and_u)
  (func (export "i32.atomic.rmw16.and_u") (result i32)
    i32.const 88
    i32.const 70000
    i32.atomic.rmw16.and_u)
  (func (export "i64.atomic.rmw8.and_u") (result i64)
    i32.const 96
    i64.const 300
    i64.atomic.rmw8.and_u)
  (func (export "i64.atomic.rmw16.and_u") (result i64)
    i32.const 104
    i64.const 70000
    i64.atomic.rmw16.and_u)
  (func (export "i64.atomic.rmw32.and_u") (result i64)
    i32.const 112
    i64.const 4294967297
    i64.atomic.rmw32.and_u)

  (func (export "i32.atomic.rmw8.or_u") (result i32)
    i32.const 120
    i32.const 300
    i32.atomic.rmw8.or_u)
  (func (export "i32.atomic.rmw16.or_u") (result i32)
    i32.const 128
    i32.const 70000
    i32.atomic.rmw16.or_u)
  (func (export "i64.atomic.rmw8.or_u") (result i64)
    i32.const 136
    i64.const 300
    i64.atomic.rmw8.or_u)
  (func (export "i64.atomic.rmw16.or_u") (result i64)
    i32.const 144
    i64.const 70000
    i64.atomic.rmw16.or_u)
  (func (export "i64.atomic.rmw32.or_u") (result i64)
    i32.const 152
    i64.const 4294967297
    i64.atomic.rmw32.or_u)

  (func (export "i32.atomic.rmw8.xor_u") (result i32)
    i32.const 160
    i32.const 300
    i32.atomic.rmw8.xor_u)
  (func (export "i32.atomic.rmw16.xor_u") (result i32)
    i32.const 168
    i32.const 70000
    i32.atomic.rmw16.xor_u)
  (func (export "i64.atomic.rmw8.xor_u") (result i64)
    i32.const 176
    i64.const 300
    i64.atomic.rmw8.xor_u)
  (func (export "i64.atomic.rmw16.xor_u") (result i64)
    i32.const 184
    i64.const 70000
    i64.atomic.rmw16.xor_u)
  (func (export "i64.atomic.rmw32.xor_u") (result i64)
    i32.const 192
    i64.const 4294967297
    i64.atomic.rmw32.xor_u)

  (func (export "i32.atomic.rmw8.xchg_u") (result i32)
    i32.const 200
    i32.const 300
    i32.atomic.rmw8.xchg_u)
  (func (export "i32.atomic.rmw16.xchg_u") (result i32)
    i32.const 208
    i32.const 70000
    i32.atomic.rmw16.xchg_u)
  (func (export "i64.atomic.rmw8.xchg_u") (result i64)
    i32.const 216
    i64.const 300
    i64.atomic.rmw8.xchg_u)
  (func (export "i64.atomic.rmw16.xchg_u") (result i64)
    i32.const 224
    i64.const 70000
    i64.atomic.rmw16.xchg_u)
  (func (export "i64.atomic.rmw32.xchg_u") (result i64)
    i32.const 232
    i64.const 4294967297
    i64.atomic.rmw32.xchg_u)

  ;; The oversized expected values truncate to zero, so these exchanges
  ;; succeed while still exposing stale upper bits in the returned value.
  (func (export "i32.atomic.rmw8.cmpxchg_u") (result i32)
    i32.const 240
    i32.const 256
    i32.const 300
    i32.atomic.rmw8.cmpxchg_u)
  (func (export "i32.atomic.rmw16.cmpxchg_u") (result i32)
    i32.const 248
    i32.const 65536
    i32.const 70000
    i32.atomic.rmw16.cmpxchg_u)
  (func (export "i64.atomic.rmw8.cmpxchg_u") (result i64)
    i32.const 256
    i64.const 256
    i64.const 300
    i64.atomic.rmw8.cmpxchg_u)
  (func (export "i64.atomic.rmw16.cmpxchg_u") (result i64)
    i32.const 264
    i64.const 65536
    i64.const 70000
    i64.atomic.rmw16.cmpxchg_u)
  (func (export "i64.atomic.rmw32.cmpxchg_u") (result i64)
    i32.const 272
    i64.const 4294967296
    i64.const 4294967297
    i64.atomic.rmw32.cmpxchg_u)
)

(assert_return (invoke "i32.atomic.rmw8.add_u") (i32.const 0))
(assert_return (invoke "i32.atomic.rmw16.add_u") (i32.const 0))
(assert_return (invoke "i64.atomic.rmw8.add_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw16.add_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw32.add_u") (i64.const 0))

(assert_return (invoke "i32.atomic.rmw8.sub_u") (i32.const 0))
(assert_return (invoke "i32.atomic.rmw16.sub_u") (i32.const 0))
(assert_return (invoke "i64.atomic.rmw8.sub_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw16.sub_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw32.sub_u") (i64.const 0))

(assert_return (invoke "i32.atomic.rmw8.and_u") (i32.const 0))
(assert_return (invoke "i32.atomic.rmw16.and_u") (i32.const 0))
(assert_return (invoke "i64.atomic.rmw8.and_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw16.and_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw32.and_u") (i64.const 0))

(assert_return (invoke "i32.atomic.rmw8.or_u") (i32.const 0))
(assert_return (invoke "i32.atomic.rmw16.or_u") (i32.const 0))
(assert_return (invoke "i64.atomic.rmw8.or_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw16.or_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw32.or_u") (i64.const 0))

(assert_return (invoke "i32.atomic.rmw8.xor_u") (i32.const 0))
(assert_return (invoke "i32.atomic.rmw16.xor_u") (i32.const 0))
(assert_return (invoke "i64.atomic.rmw8.xor_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw16.xor_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw32.xor_u") (i64.const 0))

(assert_return (invoke "i32.atomic.rmw8.xchg_u") (i32.const 0))
(assert_return (invoke "i32.atomic.rmw16.xchg_u") (i32.const 0))
(assert_return (invoke "i64.atomic.rmw8.xchg_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw16.xchg_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw32.xchg_u") (i64.const 0))

(assert_return (invoke "i32.atomic.rmw8.cmpxchg_u") (i32.const 0))
(assert_return (invoke "i32.atomic.rmw16.cmpxchg_u") (i32.const 0))
(assert_return (invoke "i64.atomic.rmw8.cmpxchg_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw16.cmpxchg_u") (i64.const 0))
(assert_return (invoke "i64.atomic.rmw32.cmpxchg_u") (i64.const 0))
