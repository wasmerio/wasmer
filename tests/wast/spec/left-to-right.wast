(module
  (memory 1)

  (type $i32_T (func (param i32 i32) (result i32)))
  (type $i64_T (func (param i64 i64) (result i32)))
  (type $f32_T (func (param f32 f32) (result i32)))
  (type $f64_T (func (param f64 f64) (result i32)))
  (table funcref
    (elem $i32_t0 $i32_t1 $i64_t0 $i64_t1 $f32_t0 $f32_t1 $f64_t0 $f64_t1)
  )

  (func $i32_t0 (type $i32_T) (i32.const -1))
  (func $i32_t1 (type $i32_T) (i32.const -2))
  (func $i64_t0 (type $i64_T) (i32.const -1))
  (func $i64_t1 (type $i64_T) (i32.const -2))
  (func $f32_t0 (type $f32_T) (i32.const -1))
  (func $f32_t1 (type $f32_T) (i32.const -2))
  (func $f64_t0 (type $f64_T) (i32.const -1))
  (func $f64_t1 (type $f64_T) (i32.const -2))

  ;; The idea is: We reset the memory, then the instruction call $*_left,
  ;; $*_right, $*_another, $*_callee (for indirect calls), and $*_bool (when a
  ;; boolean value is needed). These functions all call bump, which shifts the
  ;; memory starting at address 8 up a byte, and then store a unique value at
  ;; address 8. Then we read the 4-byte value at address 8. It should contain
  ;; the correct sequence of unique values if the calls were evaluated in the
  ;; correct order.

  (func $reset (i32.store (i32.const 8) (i32.const 0)))

  (func $bump
    (i32.store8 (i32.const 11) (i32.load8_u (i32.const 10)))
    (i32.store8 (i32.const 10) (i32.load8_u (i32.const 9)))
    (i32.store8 (i32.const 9) (i32.load8_u (i32.const 8)))
    (i32.store8 (i32.const 8) (i32.const -3)))

  (func $get (result i32) (i32.load (i32.const 8)))

  (func $i32_left (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 1)) (i32.const 0))
  (func $i32_right (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 2)) (i32.const 1))
  (func $i32_another (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 3)) (i32.const 1))
  (func $i32_callee (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 4)) (i32.const 0))
  (func $i32_bool (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 5)) (i32.const 0))
  (func $i64_left (result i64) (call $bump) (i32.store8 (i32.const 8) (i32.const 1)) (i64.const 0))
  (func $i64_right (result i64) (call $bump) (i32.store8 (i32.const 8) (i32.const 2)) (i64.const 1))
  (func $i64_another (result i64) (call $bump) (i32.store8 (i32.const 8) (i32.const 3)) (i64.const 1))
  (func $i64_callee (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 4)) (i32.const 2))
  (func $i64_bool (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 5)) (i32.const 0))
  (func $f32_left (result f32) (call $bump) (i32.store8 (i32.const 8) (i32.const 1)) (f32.const 0))
  (func $f32_right (result f32) (call $bump) (i32.store8 (i32.const 8) (i32.const 2)) (f32.const 1))
  (func $f32_another (result f32) (call $bump) (i32.store8 (i32.const 8) (i32.const 3)) (f32.const 1))
  (func $f32_callee (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 4)) (i32.const 4))
  (func $f32_bool (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 5)) (i32.const 0))
  (func $f64_left (result f64) (call $bump) (i32.store8 (i32.const 8) (i32.const 1)) (f64.const 0))
  (func $f64_right (result f64) (call $bump) (i32.store8 (i32.const 8) (i32.const 2)) (f64.const 1))
  (func $f64_another (result f64) (call $bump) (i32.store8 (i32.const 8) (i32.const 3)) (f64.const 1))
  (func $f64_callee (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 4)) (i32.const 6))
  (func $f64_bool (result i32) (call $bump) (i32.store8 (i32.const 8) (i32.const 5)) (i32.const 0))
  (func $i32_dummy (param i32 i32))
  (func $i64_dummy (param i64 i64))
  (func $f32_dummy (param f32 f32))
  (func $f64_dummy (param f64 f64))


  (func (export "i32_call_indirect") (result i32) (call $reset) (drop (call_indirect (type $i32_T) (call $i32_left) (call $i32_right) (call $i32_callee))) (call $get))
)
