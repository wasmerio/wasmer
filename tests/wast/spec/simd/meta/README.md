# Generated SIMD Spec Tests from gen_tests.py

`gen_tests.py` builds partial SIMD spec tests using templates in `simd_*.py`.
Currently it only support following simd test files generation.

- 'simd_i8x16_cmp.wast'
- 'simd_i16x8_cmp.wast'
- 'simd_i32x4_cmp.wast'
- 'simd_f32x4_cmp.wast'
- 'simd_f64x2_cmp.wast'
- 'simd_i8x16_arith.wast'
- 'simd_i8x16_arith2.wast'
- 'simd_i16x8_arith.wast'
- 'simd_i16x8_arith2.wast'
- 'simd_i32x4_arith.wast'
- 'simd_i32x4_arith2.wast'
- 'simd_f32x4_arith.wast'
- 'simd_i64x2_arith.wast'
- 'simd_f64x2_arith.wast'
- 'simd_bitwise.wast'
- 'simd_i8x16_sat_arith.wast'
- 'simd_i16x8_sat_arith.wast'
- 'simd_f32x4.wast'
- 'simd_f64x2.wast'
- 'simd_f32x4_rounding.wast'
- 'simd_f64x2_rounding.wast'
- 'simd_f32x4_pmin_pmax.wast'
- 'simd_f64x2_pmin_pmax.wast'
- 'simd_i32x4_dot_i16x8.wast'
- 'simd_load8_lane.wast'
- 'simd_load16_lane.wast'
- 'simd_load32_lane.wast'
- 'simd_load64_lane.wast,
- 'simd_store8_lane.wast'
- 'simd_store16_lane.wast'
- 'simd_store32_lane.wast'
- 'simd_store64_lane.wast,
- 'simd_i16x8_extmul_i8x16.wast'
- 'simd_i32x4_extmul_i16x8.wast'
- 'simd_i64x2_extmul_i32x4.wast'
- 'simd_int_to_int_widen.wast'
- 'simd_i32x4_trunc_sat_f32x4.wast'
- 'simd_i32x4_trunc_sat_f64x2.wast'
- 'simd_i16x8_q15mulr_sat_s.wast',
- 'simd_i16x8_extadd_pairwise_i8x16.wast',
- 'simd_i32x4_extadd_pairwise_i16x8.wast',


Usage:

```
$ python gen_tests.py -a
```

This script requires Python 3.6+, more details are documented in `gen_tests.py`.
