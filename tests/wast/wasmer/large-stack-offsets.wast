;; Stack loads must handle displacements outside the signed 12-bit range.
(module
  ;; The locals put spilled operands below fp - 2048. Keep enough copies
  ;; live to exhaust registers, then consume them with floating-point ops.
  (func (export "f32-spill") (param $x f32) (result f32)
    (local i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
    local.get $x local.get $x local.get $x local.get $x
    local.get $x local.get $x local.get $x local.get $x
    local.get $x local.get $x local.get $x local.get $x
    local.get $x local.get $x local.get $x local.get $x
    local.get $x local.get $x local.get $x local.get $x
    local.get $x local.get $x local.get $x local.get $x
    f32.add f32.add f32.add f32.add f32.add f32.add f32.add f32.add
    f32.add f32.add f32.add f32.add f32.add f32.add f32.add f32.add
    f32.add f32.add f32.add f32.add f32.add f32.add f32.add)

  ;; The final 32-bit parameters are passed above fp + 2047. Check that
  ;; their high bits survive loading and that i32 is zero-extended.
  (func $many-params
    (param i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64
           i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
    (param $i i32) (param $f f32) (result i64)
    (i64.or
      (i64.extend_i32_u (local.get $i))
      (i64.shl (i64.extend_i32_u (i32.reinterpret_f32 (local.get $f)))
               (i64.const 32))))
  (func (export "many-params") (result i64)
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0 i64.const 0
    i32.const -1
    f32.const -2
    call $many-params)
)

(assert_return (invoke "f32-spill" (f32.const 1.5)) (f32.const 36))
(assert_return (invoke "many-params") (i64.const 0xc0000000ffffffff))
