(module
  (func (export "select_i32") (param $lhs i32) (param $rhs i32) (param $cond i32) (result i32)
   (select (get_local $lhs) (get_local $rhs) (get_local $cond)))

  (func (export "select_i64") (param $lhs i64) (param $rhs i64) (param $cond i32) (result i64)
   (select (get_local $lhs) (get_local $rhs) (get_local $cond)))

  (func (export "select_f32") (param $lhs f32) (param $rhs f32) (param $cond i32) (result f32)
   (select (get_local $lhs) (get_local $rhs) (get_local $cond)))

  (func (export "select_f64") (param $lhs f64) (param $rhs f64) (param $cond i32) (result f64)
   (select (get_local $lhs) (get_local $rhs) (get_local $cond)))

  ;; Check that both sides of the select are evaluated
  (func (export "select_trap_l") (param $cond i32) (result i32)
    (select (unreachable) (i32.const 0) (get_local $cond))
  )
  (func (export "select_trap_r") (param $cond i32) (result i32)
    (select (i32.const 0) (unreachable) (get_local $cond))
  )

  (func (export "select_unreached")
    (unreachable) (select)
    (unreachable) (i32.const 0) (select)
    (unreachable) (i32.const 0) (i32.const 0) (select)
    (unreachable) (f32.const 0) (i32.const 0) (select)
    (unreachable)
  )
)

(assert_return (invoke "select_i32" (i32.const 1) (i32.const 2) (i32.const 1)) (i32.const 1))
(assert_return (invoke "select_i64" (i64.const 2) (i64.const 1) (i32.const 1)) (i64.const 2))
(assert_return (invoke "select_f32" (f32.const 1) (f32.const 2) (i32.const 1)) (f32.const 1))
(assert_return (invoke "select_f64" (f64.const 1) (f64.const 2) (i32.const 1)) (f64.const 1))

(assert_return (invoke "select_i32" (i32.const 1) (i32.const 2) (i32.const 0)) (i32.const 2))
(assert_return (invoke "select_i32" (i32.const 2) (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "select_i64" (i64.const 2) (i64.const 1) (i32.const -1)) (i64.const 2))
(assert_return (invoke "select_i64" (i64.const 2) (i64.const 1) (i32.const 0xf0f0f0f0)) (i64.const 2))

(assert_return (invoke "select_f32" (f32.const nan) (f32.const 1) (i32.const 1)) (f32.const nan))
(assert_return (invoke "select_f32" (f32.const nan:0x20304) (f32.const 1) (i32.const 1)) (f32.const nan:0x20304))
(assert_return (invoke "select_f32" (f32.const nan) (f32.const 1) (i32.const 0)) (f32.const 1))
(assert_return (invoke "select_f32" (f32.const nan:0x20304) (f32.const 1) (i32.const 0)) (f32.const 1))
(assert_return (invoke "select_f32" (f32.const 2) (f32.const nan) (i32.const 1)) (f32.const 2))
(assert_return (invoke "select_f32" (f32.const 2) (f32.const nan:0x20304) (i32.const 1)) (f32.const 2))
(assert_return (invoke "select_f32" (f32.const 2) (f32.const nan) (i32.const 0)) (f32.const nan))
(assert_return (invoke "select_f32" (f32.const 2) (f32.const nan:0x20304) (i32.const 0)) (f32.const nan:0x20304))

(assert_return (invoke "select_f64" (f64.const nan) (f64.const 1) (i32.const 1)) (f64.const nan))
(assert_return (invoke "select_f64" (f64.const nan:0x20304) (f64.const 1) (i32.const 1)) (f64.const nan:0x20304))
(assert_return (invoke "select_f64" (f64.const nan) (f64.const 1) (i32.const 0)) (f64.const 1))
(assert_return (invoke "select_f64" (f64.const nan:0x20304) (f64.const 1) (i32.const 0)) (f64.const 1))
(assert_return (invoke "select_f64" (f64.const 2) (f64.const nan) (i32.const 1)) (f64.const 2))
(assert_return (invoke "select_f64" (f64.const 2) (f64.const nan:0x20304) (i32.const 1)) (f64.const 2))
(assert_return (invoke "select_f64" (f64.const 2) (f64.const nan) (i32.const 0)) (f64.const nan))
(assert_return (invoke "select_f64" (f64.const 2) (f64.const nan:0x20304) (i32.const 0)) (f64.const nan:0x20304))

(assert_trap (invoke "select_trap_l" (i32.const 1)) "unreachable executed")
(assert_trap (invoke "select_trap_l" (i32.const 0)) "unreachable executed")
(assert_trap (invoke "select_trap_r" (i32.const 1)) "unreachable executed")
(assert_trap (invoke "select_trap_r" (i32.const 0)) "unreachable executed")

(assert_invalid
  (module (func $arity-0 (select (nop) (nop) (i32.const 1))))
  "type mismatch"
)
