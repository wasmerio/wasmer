#!/usr/bin/env python3

"""
This script is used for generating WebAssembly SIMD test cases.
It requires Python 3.6+.
"""
import sys
import argparse
import importlib


SUBMODULES = (
    'simd_i8x16_cmp',
    'simd_i16x8_cmp',
    'simd_i32x4_cmp',
    'simd_i64x2_cmp',
    'simd_f32x4_cmp',
    'simd_f64x2_cmp',
    'simd_i8x16_arith',
    'simd_i16x8_arith',
    'simd_i32x4_arith',
    'simd_f32x4_arith',
    'simd_i64x2_arith',
    'simd_f64x2_arith',
    'simd_sat_arith',
    'simd_bitwise',
    'simd_f32x4',
    'simd_f64x2',
    'simd_int_arith2',
    'simd_f32x4_rounding',
    'simd_f64x2_rounding',
    'simd_f32x4_pmin_pmax',
    'simd_f64x2_pmin_pmax',
    'simd_i32x4_dot_i16x8',
    'simd_load_lane',
    'simd_store_lane',
    'simd_ext_mul',
    'simd_int_to_int_extend',
    'simd_int_trunc_sat_float',
    'simd_i16x8_q15mulr_sat_s',
    'simd_extadd_pairwise',
)


def gen_group_tests(mod_name):
    """mod_name is the back-end script name without the.py extension.
    There must be a gen_test_cases() function in each module."""
    mod = importlib.import_module(mod_name)
    mod.gen_test_cases()


def main():
    """
    Default program entry
    """

    parser = argparse.ArgumentParser(
        description='Front-end script to call other modules to generate SIMD tests')
    parser.add_argument('-a', '--all', dest='gen_all', action='store_true',
                        default=False, help='Generate all the tests')
    parser.add_argument('-i', '--inst', dest='inst_group', choices=SUBMODULES,
                        help='Back-end scripts that generate the SIMD tests')
    args = parser.parse_args()

    if len(sys.argv) < 2:
        parser.print_help()

    if args.inst_group:
        gen_group_tests(args.inst_group)
    if args.gen_all:
        for mod_name in SUBMODULES:
            gen_group_tests(mod_name)


if __name__ == '__main__':
    main()
    print('Done.')
