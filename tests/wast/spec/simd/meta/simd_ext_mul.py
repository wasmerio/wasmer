#!/usr/bin/env python3

""" Base class for generating extended multiply instructions.  These
instructions 2 inputs of the same (narrower) lane shape, multiplies
corresponding lanes with extension (no overflow/wraparound), producing 1 output
of a (wider) shape. These instructions can choose to work on the low or high
halves of the inputs, and perform signed or unsigned multiply.

Subclasses need to define 3 attributes:
  - LANE_TYPE (this is the output shape)
  - SRC_LANE_TYPE (this is the input (narrower) shape)
  - BINARY_OPS (list of operations)
"""

from simd_arithmetic import SimdArithmeticCase


class SimdExtMulCase(SimdArithmeticCase):
    UNARY_OPS = ()

    @property
    def full_bin_test_data(self):
        return []

    def get_combine_cases(self):
        return ''

    @property
    def bin_test_data(self):
        lane_forms = [self.SRC_LANE_TYPE, self.SRC_LANE_TYPE, self.LANE_TYPE]
        return [(self.normal_binary_op_test_data, lane_forms)]

    @property
    def hex_binary_op_test_data(self):
        return []

    def gen_test_cases(self):
        wast_filename = '../simd_{wide}_extmul_{narrow}.wast'.format(
                wide=self.LANE_TYPE, narrow=self.SRC_LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())


class SimdI16x8ExtMulCase(SimdExtMulCase):
    LANE_TYPE = 'i16x8'
    SRC_LANE_TYPE = 'i8x16'
    BINARY_OPS = ('extmul_low_i8x16_s', 'extmul_high_i8x16_s',
                  'extmul_low_i8x16_u', 'extmul_high_i8x16_u')


class SimdI32x4ExtMulCase(SimdExtMulCase):
    LANE_TYPE = 'i32x4'
    SRC_LANE_TYPE = 'i16x8'
    BINARY_OPS = ('extmul_low_i16x8_s', 'extmul_high_i16x8_s',
                  'extmul_low_i16x8_u', 'extmul_high_i16x8_u')


class SimdI64x2ExtMulCase(SimdExtMulCase):
    LANE_TYPE = 'i64x2'
    SRC_LANE_TYPE = 'i32x4'
    BINARY_OPS = ('extmul_low_i32x4_s', 'extmul_high_i32x4_s',
                  'extmul_low_i32x4_u', 'extmul_high_i32x4_u')


def gen_test_cases():
    simd_i16x8_ext_mul_case = SimdI16x8ExtMulCase()
    simd_i16x8_ext_mul_case.gen_test_cases()
    simd_i32x4_ext_mul_case = SimdI32x4ExtMulCase()
    simd_i32x4_ext_mul_case.gen_test_cases()
    simd_i64x2_ext_mul_case = SimdI64x2ExtMulCase()
    simd_i64x2_ext_mul_case.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
