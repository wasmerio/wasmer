#!/usr/bin/env python3

"""
Generate f64x2 [pmin, pmax] cases.
"""

from simd_f32x4_pmin_pmax import Simdf32x4PminPmaxCase
from simd_f64x2_arith import Simdf64x2ArithmeticCase
from simd_float_op import FloatingPointSimpleOp
from simd import SIMD
from test_assert import AssertReturn


class Simdf64x2PminPmaxCase(Simdf32x4PminPmaxCase):
    LANE_TYPE = 'f64x2'
    FLOAT_NUMBERS = Simdf64x2ArithmeticCase.FLOAT_NUMBERS
    LITERAL_NUMBERS = Simdf64x2ArithmeticCase.LITERAL_NUMBERS
    NAN_NUMBERS = Simdf64x2ArithmeticCase.NAN_NUMBERS


def gen_test_cases():
    simd_f64x2_pmin_pmax_case = Simdf64x2PminPmaxCase()
    simd_f64x2_pmin_pmax_case.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
