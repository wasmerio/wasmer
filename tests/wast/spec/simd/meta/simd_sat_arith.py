#!/usr/bin/env python3

"""
Generate saturating integer arithmetic operation cases.
"""

from simd_arithmetic import SimdArithmeticCase
from test_assert import AssertReturn
from simd import SIMD


class SimdSaturateArithmeticCases(SimdArithmeticCase):
    UNARY_OPS = ()
    BINARY_OPS = ('add_sat_s', 'add_sat_u',
                  'sub_sat_s', 'sub_sat_u')
    malformed_template = '(assert_malformed (module quote\n    "(func (result v128) ' \
                         '({lane_type}.{op} ({operand_1}) ({operand_2})))")\n    "unknown operator")'

    def gen_test_cases(self):
        wast_filename = '../simd_{lane_type}_sat_arith.wast'.format(lane_type=self.LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())

    def gen_test_template(self):
        return super().gen_test_template().replace('{invalid_cases}',
                                                   '{malformed_cases}\n\n{invalid_cases}')

    def v128_const(self, lane, value, lane_len=None):
        if not lane_len:
            lane_len = self.LANE_LEN

        return 'v128.const {lane_type} {value}'.format(lane_type=lane, value=' '.join([str(value)] * lane_len))

    def get_malformed_cases(self):
        malformed_cases = [';; Malformed cases: non-existent op names']
        inst_ops = ['add', 'sub', 'mul', 'div']

        # The op names should contain _s or _u suffixes, there is no mul or div
        # for saturating integer arithmetic operation
        for op in inst_ops:
            malformed_cases.append(self.malformed_template.format(
                lane_type=self.LANE_TYPE, op='_'.join([op, 'sat']),
                operand_1=self.v128_const(self.LANE_TYPE, '1'), operand_2=self.v128_const(self.LANE_TYPE, '2')))

        return '\n'.join(malformed_cases)

    def argument_empty_cases(self):
        """Test cases with empty argument.
        """
        cases = []

        case_data = {
            'op': '',
            'extended_name': 'arg-empty',
            'param_type': '',
            'result_type': '(result v128)',
            'params': '',
        }

        for op in self.BINARY_OPS:
            case_data['op'] = '{lane_type}.{op}'.format(lane_type=self.LANE_TYPE, op=op)
            case_data['extended_name'] = '1st-arg-empty'
            case_data['params'] = SIMD.v128_const('0', self.LANE_TYPE)
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

            case_data['extended_name'] = 'arg-empty'
            case_data['params'] = ''
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

        return '\n'.join(cases)

    def get_all_cases(self):
        case_data = {'lane_type': self.LANE_TYPE,
                     'normal_cases': self.get_normal_case(),
                     'malformed_cases': self.get_malformed_cases(),
                     'invalid_cases': self.get_invalid_cases(),
                     'combine_cases': self.get_combine_cases()
                     }
        return self.gen_test_template().format(**case_data)

    @property
    def combine_ternary_arith_test_data(self):
        return {
            'sat-add_s-sub_s': [
                [str(self.lane.quarter)] * self.LANE_LEN,
                [str(self.lane.max)] * self.LANE_LEN,
                [str(self.lane.min)] * self.LANE_LEN,
                [str(self.lane.min)] * self.LANE_LEN
            ],
            'sat-add_s-sub_u': [
                [str(self.lane.mask)] * self.LANE_LEN,
                [str(self.lane.min)] * self.LANE_LEN,
                [str(self.lane.min)] * self.LANE_LEN,
                ['-1'] * self.LANE_LEN
            ],
            'sat-add_u-sub_s': [
                [str(self.lane.max)] * self.LANE_LEN,
                ['-1'] * self.LANE_LEN,
                [str(self.lane.max)] * self.LANE_LEN,
                [str(self.lane.mask - 1)] * self.LANE_LEN
            ],
            'sat-add_u-sub_u': [
                [str(self.lane.mask)] * self.LANE_LEN,
                ['0'] * self.LANE_LEN,
                ['1'] * self.LANE_LEN,
                [str(self.lane.mask)] * self.LANE_LEN
            ]
        }

    @property
    def combine_binary_arith_test_data(self):
        return {
            'sat-add_s-neg': [
                [str(self.lane.min)] * self.LANE_LEN,
                [str(self.lane.max)] * self.LANE_LEN,
                ['-1'] * self.LANE_LEN
            ],
            'sat-add_u-neg': [
                [str(self.lane.max)] * self.LANE_LEN,
                [str(self.lane.min)] * self.LANE_LEN,
                [str(self.lane.mask)] * self.LANE_LEN
            ],
            'sat-sub_s-neg': [
                [str(self.lane.min)] * self.LANE_LEN,
                [str(self.lane.max)] * self.LANE_LEN,
                [str(self.lane.min)] * self.LANE_LEN
            ],
            'sat-sub_u-neg': [
                [str(self.lane.max)] * self.LANE_LEN,
                [str(self.lane.min)] * self.LANE_LEN,
                ['1'] * self.LANE_LEN
            ]
        }

    def get_combine_cases(self):
        combine_cases = [';; combination\n(module']
        ternary_func_template = '  (func (export "{func}") (param v128 v128 v128) (result v128)\n' \
                              '    ({lane}.{op1} ({lane}.{op2} (local.get 0) (local.get 1))'\
                              '(local.get 2)))'
        for func in sorted(self.combine_ternary_arith_test_data):
            func_parts = func.split('-')
            op1 = func_parts[1].replace('_', '_sat_')
            op2 = func_parts[2].replace('_', '_sat_')
            combine_cases.append(ternary_func_template.format(func=func,
                                                            lane=self.LANE_TYPE,
                                                            op1=op1,
                                                            op2=op2))
        binary_func_template = '  (func (export "{func}") (param v128 v128) (result v128)\n'\
                             '    ({lane}.{op1} ({lane}.{op2} (local.get 0)) (local.get 1)))'
        for func in sorted(self.combine_binary_arith_test_data):
            func_parts = func.split('-')
            op1 = func_parts[1].replace('_', '_sat_')
            combine_cases.append(binary_func_template.format(func=func,
                                                           lane=self.LANE_TYPE,
                                                           op1=op1,
                                                           op2=func_parts[2]))
        combine_cases.append(')\n')

        for func, test in sorted(self.combine_ternary_arith_test_data.items()):
            combine_cases.append(str(AssertReturn(func,
                                 [SIMD.v128_const(elem, self.LANE_TYPE) for elem in test[:-1]],
                                 SIMD.v128_const(test[-1], self.LANE_TYPE))))

        for func, test in sorted(self.combine_binary_arith_test_data.items()):
            combine_cases.append(str(AssertReturn(func,
                                 [SIMD.v128_const(elem, self.LANE_TYPE) for elem in test[:-1]],
                                 SIMD.v128_const(test[-1], self.LANE_TYPE))))

        return '\n'.join(combine_cases)


class SimdI8x16SaturateArithmeticCases(SimdSaturateArithmeticCases):
    LANE_LEN = 16
    LANE_TYPE = 'i8x16'

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
    def i8x16_f32x4_test_data(self):
        return {
            'i8x16.add_sat_s': [
                [['0x80', '-0.0'], '0x80', ['i8x16', 'f32x4', 'i8x16']],
                [['1', '+inf'], ['0x01', '0x01', '0x81', '0x7f'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-inf'], ['0x01', '0x01', '0x81', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', 'nan'], ['0x01', '0x01', '0xc1', '0x7f'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-nan'], ['0x01', '0x01', '0xc1', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']]
            ],
            'i8x16.add_sat_u': [
                [['0x80', '-0.0'], ['0x80', '0x80', '0x80', '0xff'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '+inf'], ['0x01', '0x01', '0x81', '0x80'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-inf'], ['0x01', '0x01', '0x81', '0xff'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', 'nan'], ['0x01', '0x01', '0xc1', '0x80'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-nan'], ['0x01', '0x01', '0xc1', '0xff'] * 4, ['i8x16', 'f32x4', 'i8x16']],
            ],
            'i8x16.sub_sat_s': [
                [['0x80', '-0.0'], ['0x80', '0x80', '0x80', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '+inf'], ['0x01', '0x01', '0x7f', '0x82'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-inf'], ['0x01', '0x01', '0x7f', '0x02'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', 'nan'], ['0x01', '0x01', '0x41', '0x82'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-nan'], ['0x01', '0x01', '0x41', '0x02'] * 4, ['i8x16', 'f32x4', 'i8x16']],
            ],
            'i8x16.sub_sat_u': [
                [['0x80', '-0.0'], ['0x80', '0x80', '0x80', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '+inf'], ['0x01', '0x01', '0', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-inf'], ['0x01', '0x01', '0', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', 'nan'], ['0x01', '0x01', '0', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
                [['1', '-nan'], ['0x01', '0x01', '0', '0'] * 4, ['i8x16', 'f32x4', 'i8x16']],
            ]
        }

    @property
    def combine_dec_hex_test_data(self):
        return {
            'i8x16.add_sat_s': [
                [[['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15'],
                  ['0', '0xff', '0xfe', '0xfd', '0xfc', '0xfb', '0xfa', '0xf9', '0xf8', '0xf7', '0xf6', '0xf5',
                   '0xf4', '0xf3', '0xf2', '0xf1']],
                 ['0'] * 16, ['i8x16', 'i8x16', 'i8x16']]
            ],
            'i8x16.add_sat_u': [
                [[['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15'],
                  ['0', '0xff', '0xfe', '0xfd', '0xfc', '0xfb', '0xfa', '0xf9', '0xf8', '0xf7', '0xf6', '0xf5',
                   '0xf4', '0xf3', '0xf2', '0xf1']],
                 ['0'] + ['0xff'] * 15, ['i8x16', 'i8x16', 'i8x16']]
            ],
            'i8x16.sub_sat_s': [
                [[['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15'],
                  ['0', '0xff', '0xfe', '0xfd', '0xfc', '0xfb', '0xfa', '0xf9', '0xf8', '0xf7', '0xf6', '0xf5',
                   '0xf4', '0xf3', '0xf2', '0xf1']],
                 ['0', '0x02', '0x04', '0x06', '0x08', '0x0a', '0x0c', '0x0e', '0x10', '0x12', '0x14', '0x16',
                  '0x18', '0x1a', '0x1c', '0x1e'],
                 ['i8x16', 'i8x16', 'i8x16']]
            ],
            'i8x16.sub_sat_u': [
                [[['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '10', '11', '12', '13', '14', '15'],
                  ['0', '0xff', '0xfe', '0xfd', '0xfc', '0xfb', '0xfa', '0xf9', '0xf8', '0xf7', '0xf6', '0xf5',
                   '0xf4', '0xf3', '0xf2', '0xf1']],
                 ['0'] * 16,
                 ['i8x16', 'i8x16', 'i8x16']]
            ],
        }

    @property
    def range_test_data(self):
        return {
            'i8x16.add_sat_s': [
                [[[str(i) for i in range(16)], [str(i * 2) for i in range(16)]],
                 [str(i * 3) for i in range(16)], ['i8x16', 'i8x16', 'i8x16']]
            ],
            'i8x16.add_sat_u': [
                [[[str(i) for i in range(16)], [str(i * 2) for i in range(16)]],
                 [str(i * 3) for i in range(16)], ['i8x16', 'i8x16', 'i8x16']]
            ],
            'i8x16.sub_sat_s': [
                [[[str(i) for i in range(16)], [str(i * 2) for i in range(16)]],
                 [str(-i) for i in range(16)], ['i8x16', 'i8x16', 'i8x16']]
            ],
            'i8x16.sub_sat_u': [
                [[[str(i) for i in range(16)], [str(i * 2) for i in range(16)]],
                 ['0'] * 16, ['i8x16', 'i8x16', 'i8x16']]
            ],
        }

    @property
    def full_bin_test_data(self):
        return [
            self.i8x16_f32x4_test_data,
            self.combine_dec_hex_test_data,
            self.range_test_data
        ]

    def get_malformed_cases(self):
        malformed_cases = []
        # There is no saturating integer arithmetic operation for i32x4 or f32x4.
        for prefix in ['i32x4', 'f32x4']:
            for op in ['add', 'sub']:
                for suffix in ['s', 'u']:
                    malformed_cases.append(self.malformed_template.format(
                        lane_type=prefix, op='_'.join([op, 'sat', suffix]),
                        operand_1=self.v128_const(prefix, '0', lane_len=4),
                        operand_2=self.v128_const(prefix, '0', lane_len=4)
                    ))
        return super().get_malformed_cases() + '\n' + '\n'.join(malformed_cases)


class SimdI16x8SaturateArithmeticCases(SimdSaturateArithmeticCases):
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
            'i16x8.add_sat_s': [
                [['012_345', '032_123'], '032_767', ['i16x8'] * 3],
                [['012_345', '056_789'], '03_598', ['i16x8'] * 3],
                [['0x0_1234', '0x0_5678'], '0x0_68ac', ['i16x8'] * 3],
                [['0x0_90AB', '0x0_cdef'], '-0x0_8000', ['i16x8'] * 3]
            ],
            'i16x8.add_sat_u': [
                [['012_345', '056_789'], '065_535', ['i16x8'] * 3],
                [['012_345', '-012_345'], '065_535', ['i16x8'] * 3],
                [['0x0_1234', '0x0_5678'], '0x0_68ac', ['i16x8'] * 3],
                [['0x0_90AB', '0x0_cdef'], '0x0_ffff', ['i16x8'] * 3]
            ],
            'i16x8.sub_sat_s': [
                [['012_345', '056_789'], '021_092', ['i16x8'] * 3],
                [['012_345', '-012_345'], '024_690', ['i16x8'] * 3],
                [['0x0_1234', '0x0_5678'], '0x0_bbbc', ['i16x8'] * 3],
                [['0x0_90AB', '-0x1234'], '0xa2df', ['i16x8'] * 3]
            ],
            'i16x8.sub_sat_u': [
                [['012_345', '056_789'], '0', ['i16x8'] * 3],
                [['056_789', '-12_345'], '03_598', ['i16x8'] * 3],
                [['0x0_1234', '-0x0_5678'], '0', ['i16x8'] * 3],
                [['0x0_cdef', '0x0_90AB'], '0x0_3d44', ['i16x8'] * 3]
            ]
        }

    @property
    def i16x8_f32x4_test_data(self):
        return {
            'i16x8.add_sat_s': [
                [['0x8000', '-0.0'], '0x8000', ['i16x8', 'f32x4', 'i16x8']],
                [['1', '+inf'], ['0x01', '0x7f81'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-inf'], ['0x01', '0xff81'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', 'nan'], ['0x01', '0x7fc1'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-nan'], ['0x01', '0xffc1'] * 4, ['i16x8', 'f32x4', 'i16x8']]
            ],
            'i16x8.add_sat_u': [
                [['0x8000', '-0.0'], ['0x8000', '0xffff'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '+inf'], ['0x01', '0x7f81'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-inf'], ['0x01', '0xff81'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', 'nan'], ['0x01', '0x7fc1'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', 'nan'], ['0x01', '0x7fc1'] * 4, ['i16x8', 'f32x4', 'i16x8']]
            ],
            'i16x8.sub_sat_s': [
                [['0x8000', '-0.0'], ['0x8000', '0'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '+inf'], ['0x01', '0x8081'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-inf'], ['0x01', '0x81'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', 'nan'], ['0x01', '0x8041'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-nan'], ['0x01', '0x41'] * 4, ['i16x8', 'f32x4', 'i16x8']]
            ],
            'i16x8.sub_sat_u': [
                [['0x8000', '-0.0'], ['0x8000', '0'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '+inf'], ['0x01', '0'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-inf'], ['0x01', '0'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', 'nan'], ['0x01', '0'] * 4, ['i16x8', 'f32x4', 'i16x8']],
                [['1', '-nan'], ['0x01', '0'] * 4, ['i16x8', 'f32x4', 'i16x8']]
            ],
        }

    @property
    def combine_dec_hex_test_data(self):
        return {
            'i16x8.add_sat_s': [
                [[['0', '1', '2', '3', '4', '5', '6', '7'],
                  ['0', '0xffff', '0xfffe', '0xfffd', '0xfffc', '0xfffb', '0xfffa', '0xfff9']],
                 ['0'] * 8, ['i16x8'] * 3]
            ],
            'i16x8.add_sat_u': [
                [[['0', '1', '2', '3', '4', '5', '6', '7'],
                  ['0', '0xffff', '0xfffe', '0xfffd', '0xfffc', '0xfffb', '0xfffa', '0xfff9']],
                 ['0'] + ['0xffff'] * 7, ['i16x8'] * 3]
            ],
            'i16x8.sub_sat_s': [
                [[['0', '1', '2', '3', '4', '5', '6', '7'],
                  ['0', '0xffff', '0xfffe', '0xfffd', '0xfffc', '0xfffb', '0xfffa', '0xfff9']],
                 ['0', '2', '4', '6', '8', '10', '12', '14'], ['i16x8'] * 3]
            ],
            'i16x8.sub_sat_u': [
                [[['0', '1', '2', '3', '4', '5', '6', '7'],
                  ['0', '0xffff', '0xfffe', '0xfffd', '0xfffc', '0xfffb', '0xfffa', '0xfff9']],
                 ['0'] * 8, ['i16x8'] * 3]
            ]
        }

    @property
    def range_test_data(self):
        return {
            'i16x8.add_sat_s': [
                [[[str(i) for i in range(8)], [str(i * 2) for i in range(8)]],
                 [str(i * 3) for i in range(8)], ['i16x8'] * 3]
            ],
            'i16x8.add_sat_u': [
                [[[str(i) for i in range(8)], [str(i * 2) for i in range(8)]],
                 [str(i * 3) for i in range(8)], ['i16x8'] * 3]
            ],
            'i16x8.sub_sat_s': [
                [[[str(i) for i in range(8)], [str(i * 2) for i in range(8)]],
                 [str(-i) for i in range(8)], ['i16x8'] * 3]
            ],
            'i16x8.sub_sat_u': [
                [[[str(i) for i in range(8)], [str(i * 2) for i in range(8)]],
                 ['0'] * 8, ['i16x8'] * 3]
            ]
        }

    @property
    def full_bin_test_data(self):
        return [
            self.i16x8_f32x4_test_data,
            self.combine_dec_hex_test_data,
            self.range_test_data,
            self.underscore_literal_test_data
        ]


def gen_test_cases():
    simd_i8x16_sat_arith = SimdI8x16SaturateArithmeticCases()
    simd_i8x16_sat_arith.gen_test_cases()
    simd_i16x8_sat_arith = SimdI16x8SaturateArithmeticCases()
    simd_i16x8_sat_arith.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
