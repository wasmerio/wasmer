(module
  (func (export "promote") (result i64)
    f32.const -nan:0x400001
    f32.const 1
    f32.add
    f64.promote_f32
    i64.reinterpret_f64)
  (func (export "promote-neg") (result i64)
    f32.const nan:0x400001
    f32.const 1
    f32.add
    f64.promote_f32
    f64.neg
    i64.reinterpret_f64)
  (func (export "demote") (result i32)
    f64.const -nan:0x8000020000000
    f64.const 1
    f64.add
    f32.demote_f64
    i32.reinterpret_f32)
  (func (export "demote-neg") (result i32)
    f64.const nan:0x8000020000000
    f64.const 1
    f64.add
    f32.demote_f64
    f32.neg
    i32.reinterpret_f32)
)

(assert_return (invoke "promote") (i64.const 0x7ff8000000000000))
(assert_return (invoke "promote-neg") (i64.const 0xfff8000000000000))
(assert_return (invoke "demote") (i32.const 0x7fc00000))
(assert_return (invoke "demote-neg") (i32.const 0xffc00000))

(module
  (func $f64-to-f32 (param f64) (result f32) (f32.const 0))
  (func $f32-to-f64 (param f32) (result f64) (f64.const 0))
  ;; Keep enough float values live to spill the call argument. Its slot is
  ;; reused for the result, which must not inherit the argument's metadata.
  (func (export "call-promote") (result f64) (local $r f64)
    (f64.add (f64.const 1) (f64.const 2))
    (f64.add (f64.const 1) (f64.const 2))
    (f64.add (f64.const 1) (f64.const 2))
    (f64.add (f64.const 1) (f64.const 2))
    (f64.add (f64.const 1) (f64.const 2))
    (f64.ceil (f64.const 1.5))
    (call $f64-to-f32)
    (f64.promote_f32)
    (local.set $r)
    (drop) (drop) (drop) (drop) (drop)
    (local.get $r))
  (func (export "call-demote") (result f32) (local $r f32)
    (f32.add (f32.const 1) (f32.const 2))
    (f32.add (f32.const 1) (f32.const 2))
    (f32.add (f32.const 1) (f32.const 2))
    (f32.add (f32.const 1) (f32.const 2))
    (f32.add (f32.const 1) (f32.const 2))
    (f32.ceil (f32.const 1.5))
    (call $f32-to-f64)
    (f32.demote_f64)
    (local.set $r)
    (drop) (drop) (drop) (drop) (drop)
    (local.get $r))
)

(assert_return (invoke "call-promote") (f64.const 0))
(assert_return (invoke "call-demote") (f32.const 0))
