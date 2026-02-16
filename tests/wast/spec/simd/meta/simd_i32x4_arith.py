#!/usr/bin/env python3

"""
Generate i32x4 integer arithmetic operation cases.
"""

from simd_arithmetic import SimdArithmeticCase


class SimdI32x4ArithmeticCase(SimdArithmeticCase):

    LANE_LEN = 4
    LANE_TYPE = 'i32x4'

    @property
    def hex_binary_op_test_data(self):
        return [
            ('0x3fffffff', '0x40000000'),
            ('0x40000000', '0x40000000'),
            ('-0x3fffffff', '-0x40000000'),
            ('-0x40000000', '-0x40000000'),
            ('-0x40000000', '-0x40000001'),
            ('0x7fffffff', '0x7fffffff'),
            ('0x7fffffff', '0x01'),
            ('0x80000000', '-0x01'),
            ('0x7fffffff', '0x80000000'),
            ('0x80000000', '0x80000000'),
            ('0xffffffff', '0x01'),
            ('0xffffffff', '0xffffffff')
        ]

    @property
    def hex_unary_op_test_data(self):
        return ['0x01', '-0x01', '-0x80000000', '-0x7fffffff', '0x7fffffff', '0x80000000', '0xffffffff']

    @property
    def underscore_literal_test_data(self):
        return {
            'i32x4.add': [
                [['01_234_567_890', '01_234_567_890'], '02_469_135_780', ['i32x4'] * 3],
                [['0x0_1234_5678', '0x0_90AB_cdef'], '0x0_a2e0_2467', ['i32x4'] * 3]
            ],
            'i32x4.sub': [
                [['03_214_567_890 ', '01_234_567_890 '], '01_980_000_000', ['i32x4'] * 3],
                [['0x0_90AB_cdef', '0x0_1234_5678'], '0x0_7e77_7777', ['i32x4'] * 3]
            ],
            'i32x4.mul': [
                [['0_123_456_789', '0_987_654_321'], '04_227_814_277', ['i32x4'] * 3],
                [['0x0_1234_5678', '0x0_90AB_cdef'], '0x0_2a42_d208', ['i32x4'] * 3]
            ]
        }

    @property
    def i32x4_i8x16_test_data(self):
        return {
            'i32x4.add': [
                [['0x7fffffff', ['0', '0', '0', '0x80'] * 4], '-1', ['i32x4', 'i8x16', 'i32x4']],
                [['1', '255'], '0', ['i32x4', 'i8x16', 'i32x4']]
            ],
            'i32x4.sub': [
                [['0x7fffffff', ['0', '0', '0', '0x80'] * 4], '-1', ['i32x4', 'i8x16', 'i32x4']],
                [['1', '255'], '2', ['i32x4', 'i8x16', 'i32x4']]
            ],
            'i32x4.mul': [
                [['0x10000000', '0x10'], '0', ['i32x4', 'i8x16', 'i32x4']],
                [['0xffffffff', '255'], '1', ['i32x4', 'i8x16', 'i32x4']]
            ]
        }

    @property
    def i32x4_i16x8_test_data(self):
        return {
            'i32x4.add': [
                [['0x7fffffff', ['0', '0x8000'] * 4], '-1', ['i32x4', 'i16x8', 'i32x4']],
                [['1', '0xffff'], '0', ['i32x4', 'i16x8', 'i32x4']]
            ],
            'i32x4.sub': [
                [['0x7fffffff', ['0', '0x8000'] * 4], '-1', ['i32x4', 'i16x8', 'i32x4']],
                [['1', '0xffff'], '0x02', ['i32x4', 'i16x8', 'i32x4']]
            ],
            'i32x4.mul': [
                [['0x80000000', ['0', '0x02'] * 4], '0', ['i32x4', 'i16x8', 'i32x4']],
                [['0xffffffff', '0xffff'], '1', ['i32x4', 'i16x8', 'i32x4']]
            ]
        }

    @property
    def i32x4_f32x4_test_data(self):
        return {
            'i32x4.add': [
                [['0x80000000', '+0.0'], '0x80000000', ['i32x4', 'f32x4', 'i32x4']],
                [['0x80000000', '-0.0'], '0', ['i32x4', 'f32x4', 'i32x4']],
                [['0x80000000', '1.0'], '0xbf800000', ['i32x4', 'f32x4', 'i32x4']],
                [['0x80000000', '-1.0'], '0x3f800000', ['i32x4', 'f32x4', 'i32x4']],
                [['1', '+inf'], '0x7f800001', ['i32x4', 'f32x4', 'i32x4']],
                [['1', '-inf'], '0xff800001', ['i32x4', 'f32x4', 'i32x4']],
                [['1', 'nan'], '0x7fc00001', ['i32x4', 'f32x4', 'i32x4']]
            ],
            'i32x4.sub': [
                [['0x80000000', '+0.0'], '0x80000000', ['i32x4', 'f32x4', 'i32x4']],
                [['0x80000000', '-0.0'], '0', ['i32x4', 'f32x4', 'i32x4']],
                [['0x80000000', '1.0'], '0x40800000', ['i32x4', 'f32x4', 'i32x4']],
                [['0x80000000', '-1.0'], '0xc0800000', ['i32x4', 'f32x4', 'i32x4']],
                [['0x1', '+inf'], '0x80800001', ['i32x4', 'f32x4', 'i32x4']],
                [['0x1', '-inf'], '0x00800001', ['i32x4', 'f32x4', 'i32x4']],
                [['0x1', 'nan'], '0x80400001', ['i32x4', 'f32x4', 'i32x4']]
            ],
            'i32x4.mul': [
                [['0x8000', '+0.0'], '0', ['i32x4', 'f32x4', 'i32x4']],
                [['0x8000', '-0.0'], '0', ['i32x4', 'f32x4', 'i32x4']],
                [['0x8000', '1.0'], '0', ['i32x4', 'f32x4', 'i32x4']],
                [['0x8000', '-1.0'], '0', ['i32x4', 'f32x4', 'i32x4']],
                [['0x1', '+inf'], '0x7f800000', ['i32x4', 'f32x4', 'i32x4']],
                [['0x1', '-inf'], '0xff800000', ['i32x4', 'f32x4', 'i32x4']],
                [['0x1', 'nan'], '0x7fc00000', ['i32x4', 'f32x4', 'i32x4']]
            ]
        }

    @property
    def combine_dec_hex_test_data(self):
        return {
            'i32x4.add': [
                [[['0', '1', '2', '3'],
                  ['0', '0xffffffff', '0xfffffffe', '0xfffffffd']],
                 ['0'] * 16, ['i32x4'] * 3]
            ],
            'i32x4.sub': [
                [[['0', '1', '2', '3'],
                  ['0', '0xffffffff', '0xfffffffe', '0xfffffffd']],
                 ['0', '0x02', '0x04', '0x06'], ['i32x4'] * 3]
            ],
            'i32x4.mul': [
                [[['0', '1', '2', '3'],
                  ['0', '0xffffffff', '0xfffffffe', '0xfffffffd']],
                 ['0', '0xffffffff', '0xfffffffc', '0xfffffff7'],
                 ['i32x4'] * 3]
            ]
        }

    @property
    def range_test_data(self):
        return {
            'i32x4.add': [
                [[[str(i) for i in range(4)], [str(i * 2) for i in range(4)]],
                 [str(i * 3) for i in range(4)], ['i32x4'] * 3]
            ],
            'i32x4.sub': [
                [[[str(i) for i in range(4)], [str(i * 2) for i in range(4)]],
                 [str(-i) for i in range(4)], ['i32x4'] * 3]
            ],
            'i32x4.mul': [
                [[[str(i) for i in range(4)], [str(i * 2) for i in range(4)]],
                 ['0', '0x02', '0x08', '0x12'],
                 ['i32x4'] * 3]
            ]
        }

    @property
    def full_bin_test_data(self):
        return [
            self.i32x4_i8x16_test_data,
            self.i32x4_i16x8_test_data,
            self.i32x4_f32x4_test_data,
            self.combine_dec_hex_test_data,
            self.range_test_data,
            self.underscore_literal_test_data
        ]


def gen_test_cases():
    simd_i32x4_arith = SimdI32x4ArithmeticCase()
    simd_i32x4_arith.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()