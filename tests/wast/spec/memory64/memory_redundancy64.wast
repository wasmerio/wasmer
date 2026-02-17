;; Test that optimizers don't do redundant-load, store-to-load, or dead-store
;; optimizations when there are interfering stores, even of different types
;; and to non-identical addresses.

(module
  (memory i64 1 1)

  (func (export "zero_everything")
    (i32.store (i64.const 0) (i32.const 0))
    (i32.store (i64.const 4) (i32.const 0))
    (i32.store (i64.const 8) (i32.const 0))
    (i32.store (i64.const 12) (i32.const 0))
  )

  (func (export "test_store_to_load") (result i32)
    (i32.store (i64.const 8) (i32.const 0))
    (f32.store (i64.const 5) (f32.const -0.0))
    (i32.load (i64.const 8))
  )

  (func (export "test_redundant_load") (result i32)
    (local $t i32)
    (local $s i32)
    (local.set $t (i32.load (i64.const 8)))
    (i32.store (i64.const 5) (i32.const 0x80000000))
    (local.set $s (i32.load (i64.const 8)))
    (i32.add (local.get $t) (local.get $s))
  )

  (func (export "test_dead_store") (result f32)
    (local $t f32)
    (i32.store (i64.const 8) (i32.const 0x23232323))
    (local.set $t (f32.load (i64.const 11)))
    (i32.store (i64.const 8) (i32.const 0))
    (local.get $t)
  )

  ;; A function named "malloc" which implementations nonetheless shouldn't
  ;; assume behaves like C malloc.
  (func $malloc (export "malloc")
     (param $size i64)
     (result i64)
     (i64.const 16)
  )

  ;; Call malloc twice, but unlike C malloc, we don't get non-aliasing pointers.
  (func (export "malloc_aliasing")
     (result i32)
     (local $x i64)
     (local $y i64)
     (local.set $x (call $malloc (i64.const 4)))
     (local.set $y (call $malloc (i64.const 4)))
     (i32.store (local.get $x) (i32.const 42))
     (i32.store (local.get $y) (i32.const 43))
     (i32.load (local.get $x))
  )
)

(assert_return (invoke "test_store_to_load") (i32.const 0x00000080))
(invoke "zero_everything")
(assert_return (invoke "test_redundant_load") (i32.const 0x00000080))
(invoke "zero_everything")
(assert_return (invoke "test_dead_store") (f32.const 0x1.18p-144))
(invoke "zero_everything")
(assert_return (invoke "malloc_aliasing") (i32.const 43))
