#!/usr/bin/env python3

"""
Generate f64x2 [ceil, floor, trunc, nearest] cases.
"""

from simd_f32x4_rounding import Simdf32x4RoundingCase
from simd_f64x2 import Simdf64x2Case
from simd_f64x2_arith import Simdf64x2ArithmeticCase
from simd_float_op import FloatingPointRoundingOp
from simd import SIMD
from test_assert import AssertReturn


class Simdf64x2RoundingCase(Simdf32x4RoundingCase):

    LANE_TYPE = 'f64x2'
    FLOAT_NUMBERS = Simdf64x2ArithmeticCase.FLOAT_NUMBERS
    LITERAL_NUMBERS = Simdf64x2ArithmeticCase.LITERAL_NUMBERS
    NAN_NUMBERS = Simdf64x2ArithmeticCase.NAN_NUMBERS


def gen_test_cases():
    simd_f64x2_case = Simdf64x2RoundingCase()
    simd_f64x2_case.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
