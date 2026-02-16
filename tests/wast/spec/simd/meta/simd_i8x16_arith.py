#!/usr/bin/env python3

"""
Generate i8x16 integer arithmetic operation cases.
"""

from simd_arithmetic import SimdArithmeticCase


class SimdI8x16ArithmeticCase(SimdArithmeticCase):

    LANE_LEN = 16
    LANE_TYPE = 'i8x16'
    BINARY_OPS = ('add', 'sub')

    @property
    def hex_binary_op_test_data(self):
        return [
            ('0x3f', '0x40'),
            ('0x40', '0x40'),
            ('-0x3f', '-0x40'),
            ('-0x40', '-0x40'),
            ('-0x40', '-0x41'),
            ('0x7f', '0x7f'),
            ('0x7f', '0x01'),
            ('0x80', '-0x01'),
            ('0x7f', '0x80'),
            ('0x80', '0x80'),
            ('0xff', '0x01'),
            ('0xff', '0xff')
        ]

    @property
    def hex_unary_op_test_data(self):
        return ['0x01', '-0x01', '-0x80', '-0x7f', '0x7f', '0x80', '0xff']

    @property
    def i8x16_i16x8_test_data(self):
        return {
            'i8x16.add': [
                [['0x7f', '0x8080'], '-1', ['i8x16', 'i16x8', 'i8x16']],
                [['1', '65535'], '0', ['i8x16', 'i16x8', 'i8x16']]
            ],
            'i8x16.sub': [
                [['0x7f', '0x8080'], '-1', ['i8x16', 'i16x8', 'i8x16']],
                [['1', '65535'], '2', ['i8x16', 'i16x8', 'i8x16']]
            ]
        }

    @property
    def i8x16_i32x4_test_data(self):
        return {
            'i8x16.add': [
                [['0x7f', '0x80808080'], '-1', ['i8x16', 'i32x4', 'i8x16']],
                [['1', '0xffffffff'], '0', ['i8x16', 'i32x4', 'i8x16']]
            ],
            'i8x16.sub': [
                [['0x7f', '0x80808080'], '-1', ['i8x16', 'i32x4', 'i8x16']],
                [['1', '0xffffffff'], '2', ['i8x16', 'i32x4', 'i8x16']]
            ]
        }

    @property
    def i8x16_f32x4_test_data(self):
        return {
            'i8x16.add': [
                [['0x80', '+0.0'], '0x80', ['i8x16', 'f32x4', 'i8x16']],
                [['0x80', '-0.0'], ['0x80', '0x80', '0x80', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['0x80', '1.0'], ['0x80', '0x80', '0', '0xbf'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['0x80', '-1.0'], ['0x80', '0x80', '0', '0x3f'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '+inf'], ['0x01', '0x01', '0x81', '0x80'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-inf'], ['0x01', '0x01', '0x81', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', 'nan'], ['0x01', '0x01', '0xc1', '0x80'] * 4, ['i8x16', 'f32x4', 'i8x16']]
            ],
            'i8x16.sub': [
                [['0x80', '+0.0'], '0x80', ['i8x16', 'f32x4', 'i8x16']],
                [['0x80', '-0.0'], ['0x80', '0x80', '0x80', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['0x80', '1.0'], ['0x80', '0x80', '0', '0x41'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['0x80', '-1.0'], ['0x80', '0x80', '0', '0xc1'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '+inf'], ['0x01', '0x01', '0x81', '0x82'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-inf'], ['0x01', '0x01', '0x81', '0x02'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', 'nan'], ['0x01', '0x01', '0x41', '0x82'] * 4, ['i8x16', 'f32x4', 'i8x16']]
            ]
        }

    @property
    def combine_dec_hex_test_data(self):
        return {
            'i8x16.add': [
                [[['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15'],
                  ['0', '0xff', '0xfe', '0xfd', '0xfc', '0xfb', '0xfa', '0xf9', '0xf8', '0xf7', '0xf6', '0xf5',
                   '0xf4', '0xf3', '0xf2', '0xf1']],
                 ['0'] * 16, ['i8x16', 'i8x16', 'i8x16']]
            ],
            'i8x16.sub': [
                [[['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15'],
                  ['0', '0xff', '0xfe', '0xfd', '0xfc', '0xfb', '0xfa', '0xf9', '0xf8', '0xf7', '0xf6', '0xf5',
                   '0xf4', '0xf3', '0xf2', '0xf1']],
                 ['0', '0x02', '0x04', '0x06', '0x08', '0x0a', '0x0c', '0x0e', '0x10', '0x12', '0x14', '0x16',
                  '0x18', '0x1a', '0x1c', '0x1e'],
                 ['i8x16', 'i8x16', 'i8x16']]
            ]
        }

    @property
    def range_test_data(self):
        return {
            'i8x16.add': [
                [[[str(i) for i in range(16)], [str(i * 2) for i in range(16)]],
                 [str(i * 3) for i in range(16)], ['i8x16', 'i8x16', 'i8x16']]
            ],
            'i8x16.sub': [
                [[[str(i) for i in range(16)], [str(i * 2) for i in range(16)]],
                 [str(-i) for i in range(16)], ['i8x16', 'i8x16', 'i8x16']]
            ]
        }

    @property
    def combine_ternary_arith_test_data(self):
        test_data = super().combine_ternary_arith_test_data
        test_data.pop('mul-add')
        test_data.pop('mul-sub')
        return test_data

    @property
    def combine_binary_arith_test_data(self):
        test_data = super().combine_binary_arith_test_data
        test_data.pop('mul-neg')
        return test_data

    @property
    def full_bin_test_data(self):
        return [
            self.i8x16_i16x8_test_data,
            self.i8x16_i32x4_test_data,
            self.i8x16_f32x4_test_data,
            self.combine_dec_hex_test_data,
            self.range_test_data
        ]


def gen_test_cases():
    simd_i8x16_arith = SimdI8x16ArithmeticCase()
    simd_i8x16_arith.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()