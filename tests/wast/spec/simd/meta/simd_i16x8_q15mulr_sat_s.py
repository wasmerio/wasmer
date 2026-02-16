#!/usr/bin/env python3

from simd_arithmetic import SimdArithmeticCase


"""Generate test cases for i16x8.mulr_sat_s
"""
class SimdI16x8Q15MulRSatS(SimdArithmeticCase):
    LANE_TYPE = 'i16x8'
    UNARY_OPS = ()
    BINARY_OPS = ('q15mulr_sat_s',)

    @property
    def full_bin_test_data(self):
        return []

    @property
    def hex_binary_op_test_data(self):
        return []

    def get_combine_cases(self):
        return ''

    def gen_test_cases(self):
        wast_filename = '../simd_i16x8_q15mulr_sat_s.wast'
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())


def gen_test_cases():
    simd_i16x8_q16mulr_sat_s = SimdI16x8Q15MulRSatS()
    simd_i16x8_q16mulr_sat_s.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
