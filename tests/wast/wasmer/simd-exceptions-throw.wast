;; Throw must canonicalize SIMD payloads even when there is no local handler.
(module
  (tag $t (param v128))

  (func (export "_main") (result i32)
    i32.const 0
    i32x4.splat
    throw $t)

  (func $i16x8 (result v128)
    i32.const 1234
    i16x8.splat
    throw $t)
  (func $i32x4 (result v128)
    i32.const 12345678
    i32x4.splat
    throw $t)
  (func $i64x2 (result v128)
    i64.const 1234567890123
    i64x2.splat
    throw $t)
  (func $f32x4 (result v128)
    f32.const 1.5
    f32x4.splat
    throw $t)
  (func $f64x2 (result v128)
    f64.const 2.5
    f64x2.splat
    throw $t)
  (func $trunc_sat (result v128)
    v128.const f32x4 -1 1.5 42 4294967296
    i32x4.trunc_sat_f32x4_u
    throw $t)

  (func (export "i16x8") (result v128)
    (block $catch (result v128)
      (try_table (result v128) (catch $t $catch)
        call $i16x8)))
  (func (export "i32x4") (result v128)
    (block $catch (result v128)
      (try_table (result v128) (catch $t $catch)
        call $i32x4)))
  (func (export "i64x2") (result v128)
    (block $catch (result v128)
      (try_table (result v128) (catch $t $catch)
        call $i64x2)))
  (func (export "f32x4") (result v128)
    (block $catch (result v128)
      (try_table (result v128) (catch $t $catch)
        call $f32x4)))
  (func (export "f64x2") (result v128)
    (block $catch (result v128)
      (try_table (result v128) (catch $t $catch)
        call $f64x2)))
  (func (export "trunc_sat") (result v128)
    (block $catch (result v128)
      (try_table (result v128) (catch $t $catch)
        call $trunc_sat))))

(assert_return (invoke "i16x8") (v128.const i16x8 1234 1234 1234 1234 1234 1234 1234 1234))
(assert_return (invoke "i32x4") (v128.const i32x4 12345678 12345678 12345678 12345678))
(assert_return (invoke "i64x2") (v128.const i64x2 1234567890123 1234567890123))
(assert_return (invoke "f32x4") (v128.const f32x4 1.5 1.5 1.5 1.5))
(assert_return (invoke "f64x2") (v128.const f64x2 2.5 2.5))
(assert_return (invoke "trunc_sat") (v128.const i32x4 0 1 42 4294967295))
