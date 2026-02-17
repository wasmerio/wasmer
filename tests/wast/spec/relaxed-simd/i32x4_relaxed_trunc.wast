;; Tests for i32x4.relaxed_trunc_f32x4_s, i32x4.relaxed_trunc_f32x4_u, i32x4.relaxed_trunc_f64x2_s_zero, and i32x4.relaxed_trunc_f64x2_u_zero.

(module
    (func (export "i32x4.relaxed_trunc_f32x4_s") (param v128) (result v128) (i32x4.relaxed_trunc_f32x4_s (local.get 0)))
    (func (export "i32x4.relaxed_trunc_f32x4_u") (param v128) (result v128) (i32x4.relaxed_trunc_f32x4_u (local.get 0)))
    (func (export "i32x4.relaxed_trunc_f64x2_s_zero") (param v128) (result v128) (i32x4.relaxed_trunc_f64x2_s_zero (local.get 0)))
    (func (export "i32x4.relaxed_trunc_f64x2_u_zero") (param v128) (result v128) (i32x4.relaxed_trunc_f64x2_u_zero (local.get 0)))
)
