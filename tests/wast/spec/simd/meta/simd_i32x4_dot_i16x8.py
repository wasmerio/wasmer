#!/usr/bin/env python3

from simd_arithmetic import SimdArithmeticCase, i16
from simd_integer_op import ArithmeticOp


class SimdI32x4DotI16x8TestCase(SimdArithmeticCase):
    LANE_TYPE = 'i32x4'
    UNARY_OPS = ()
    BINARY_OPS = ('dot_i16x8_s',)

    @property
    def lane(self):
        return i16

    def binary_op(self, x, y, lane):
        # For test data we always splat a single value to the
        # entire v128, so '* 2' will work here.
        return ArithmeticOp.get_valid_value(x, i16) * ArithmeticOp.get_valid_value(y, i16) * 2

    @property
    def hex_binary_op_test_data(self):
        return []

    @property
    def bin_test_data(self):
        return [
            (self.normal_binary_op_test_data, ['i16x8', 'i16x8', 'i32x4']),
            (self.hex_binary_op_test_data, ['i16x8', 'i16x8', 'i32x4'])
        ]

    def get_case_data(self):
        case_data = []
        op_name = 'i32x4.dot_i16x8_s'
        case_data.append(['#', op_name])
        for data_group, v128_forms in self.bin_test_data:
            for data in data_group:
                case_data.append([op_name, [str(data[0]), str(data[1])],
                    str(self.binary_op(data[0], data[1], self.lane)),
                    v128_forms])
        return case_data

    def get_combine_cases(self):
        return ''

    def gen_test_cases(self):
        wast_filename = '../simd_i32x4_dot_i16x8.wast'
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())

def gen_test_cases():
    simd_i16x8_arith = SimdI32x4DotI16x8TestCase()
    simd_i16x8_arith.gen_test_cases()

if __name__ == '__main__':
    gen_test_cases()
