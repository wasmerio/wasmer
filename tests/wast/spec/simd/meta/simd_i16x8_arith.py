#!/usr/bin/env python3

"""
Generate i16x8 integer arithmetic operation cases.
"""

from simd_arithmetic import SimdArithmeticCase


class SimdI16x8ArithmeticCase(SimdArithmeticCase):

    LANE_LEN = 8
    LANE_TYPE = 'i16x8'

    @property
    def hex_binary_op_test_data(self):
        return [
            ('0x3fff', '0x4000'),
            ('0x4000', '0x4000'),
            ('-0x3fff', '-0x4000'),
            ('-0x4000', '-0x4000'),
            ('-0x4000', '-0x4001'),
            ('0x7fff', '0x7fff'),
            ('0x7fff', '0x01'),
            ('0x8000', '-0x01'),
            ('0x7fff', '0x8000'),
            ('0x8000', '0x8000'),
            ('0xffff', '0x01'),
            ('0xffff', '0xffff')
        ]

    @property
    def hex_unary_op_test_data(self):
        return ['0x01', '-0x01', '-0x8000', '-0x7fff', '0x7fff', '0x8000', '0xffff']

    @property
    def underscore_literal_test_data(self):
        return {
            'i16x8.add': [
                [['012_345', '056_789'], '03_598', ['i16x8'] * 3],
                [['0x0_1234', '0x0_5678'], '0x0_68ac', ['i16x8'] * 3]
            ],
            'i16x8.sub': [
                [['056_789', '012_345'], '044_444', ['i16x8'] * 3],
                [['0x0_5678', '0x0_1234'], '0x0_4444', ['i16x8'] * 3]
            ],
            'i16x8.mul': [
                [['012_345', '056_789'], '021_613', ['i16x8'] * 3],
                [['0x0_1234', '0x0_cdef'], '0x0_a28c', ['i16x8'] * 3]
            ]
        }

    @property
    def i16x8_i8x16_test_data(self):
        return {
            'i16x8.add': [
                [['0x7fff', ['0', '0x80'] * 8], '-1', ['i16x8', 'i8x16', 'i16x8']],
                [['1', '255'], '0', ['i16x8', 'i8x16', 'i16x8']]
            ],
            'i16x8.sub': [
                [['0x7fff', ['0', '0x80'] * 8], '-1', ['i16x8', 'i8x16', 'i16x8']],
                [['1', '255'], '0x02', ['i16x8', 'i8x16', 'i16x8']]
            ],
            'i16x8.mul': [
                [['0x1000', '0x10'], '0', ['i16x8', 'i8x16', 'i16x8']],
                [['65535', '255'], '0x01', ['i16x8', 'i8x16', 'i16x8']]
            ]
        }

    @property
    def i16x8_i32x4_test_data(self):
        return {
            'i16x8.add': [
                [['0x7fff', '0x80008000'], '-1', ['i16x8', 'i32x4', 'i16x8']],
                [['1', '0xffffffff'], '0', ['i16x8', 'i32x4', 'i16x8']]
            ],
            'i16x8.sub': [
                [['0x7fff', '0x80008000'], '-1', ['i16x8', 'i32x4', 'i16x8']],
                [['1', '0xffffffff'], '0x02', ['i16x8', 'i32x4', 'i16x8']]
            ],
            'i16x8.mul': [
                [['0x8000', '0x00020002'], '0', ['i16x8', 'i32x4', 'i16x8']],
                [['65535', '0xffffffff'], '0x01', ['i16x8', 'i32x4', 'i16x8']]
            ]
        }

    @property
    def i16x8_f32x4_test_data(self):
        return {
            'i16x8.add': [
                [['0x8000', '+0.0'], '0x8000', ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '-0.0'], ['0x8000', '0'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '1.0'], ['0x8000', '0xbf80'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '-1.0'], ['0x8000', '0x3f80'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '+inf'], ['0x01', '0x7f81'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-inf'], ['0x01', '0xff81'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', 'nan'], ['0x01', '0x7fc1'] * 4, ['i16x8', 'f32x4', 'i16x8']]
            ],
            'i16x8.sub': [
                [['0x8000', '+0.0'], '0x8000', ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '-0.0'], ['0x8000', '0'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '1.0'], ['0x8000', '0x4080'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '-1.0'], ['0x8000', '0xc080'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '+inf'], ['0x01', '0x8081'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-inf'], ['0x01', '0x81'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', 'nan'], ['0x01', '0x8041'] * 4, ['i16x8', 'f32x4', 'i16x8']]
            ],
            'i16x8.mul': [
                [['0x8000', '+0.0'], '0', ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '-0.0'], '0', ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '1.0'], '0', ['i16x8', 'f32x4', 'i16x8']],
                [['0x8000', '-1.0'], '0', ['i16x8', 'f32x4', 'i16x8']],
                [['1', '+inf'], ['0', '0x7f80'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-inf'], ['0', '0xff80'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', 'nan'], ['0', '0x7fc0'] * 4, ['i16x8', 'f32x4', 'i16x8']]
            ]
        }

    @property
    def combine_dec_hex_test_data(self):
        return {
            'i16x8.add': [
                [[['0', '1', '2', '3', '4', '5', '6', '7'],
                  ['0', '0xffff', '0xfffe', '0xfffd', '0xfffc', '0xfffb', '0xfffa', '0xfff9']],
                 ['0'] * 8, ['i16x8'] * 3]
            ],
            'i16x8.sub': [
                [[['0', '1', '2', '3', '4', '5', '6', '7'],
                  ['0', '0xffff', '0xfffe', '0xfffd', '0xfffc', '0xfffb', '0xfffa', '0xfff9']],
                 ['0', '0x02', '0x04', '0x06', '0x08', '0x0a', '0x0c', '0x0e'], ['i16x8'] * 3]
            ],
            'i16x8.mul': [
                [[['0', '1', '2', '3', '4', '5', '6', '7'],
                  ['0', '0xffff', '0xfffe', '0xfffd', '0xfffc', '0xfffb', '0xfffa', '0xfff9']],
                 ['0', '0xffff', '0xfffc', '0xfff7', '0xfff0', '0xffe7', '0xffdc', '0xffcf'],
                 ['i16x8'] * 3]
            ]
        }

    @property
    def range_test_data(self):
        return {
            'i16x8.add': [
                [[[str(i) for i in range(8)], [str(i * 2) for i in range(8)]],
                 [str(i * 3) for i in range(8)], ['i16x8'] * 3]
            ],
            'i16x8.sub': [
                [[[str(i) for i in range(8)], [str(i * 2) for i in range(8)]],
                 [str(-i) for i in range(8)], ['i16x8'] * 3]
            ],
            'i16x8.mul': [
                [[[str(i) for i in range(8)], [str(i * 2) for i in range(8)]],
                 ['0', '0x02', '0x08', '0x12', '0x20', '0x32', '0x48', '0x62'],
                 ['i16x8'] * 3]
            ]
        }

    @property
    def full_bin_test_data(self):
        return [
            self.i16x8_i8x16_test_data,
            self.i16x8_i32x4_test_data,
            self.i16x8_f32x4_test_data,
            self.combine_dec_hex_test_data,
            self.range_test_data,
            self.underscore_literal_test_data
        ]


def gen_test_cases():
    simd_i16x8_arith = SimdI16x8ArithmeticCase()
    simd_i16x8_arith.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()