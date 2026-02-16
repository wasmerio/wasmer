#!/usr/bin/env python3

"""
Generate f32x4 floating-point arithmetic operation cases.
"""

from simd_f32x4_arith import Simdf32x4ArithmeticCase
from simd_float_op import FloatingPointArithOp


class F64ArithOp(FloatingPointArithOp):
    maximum = '0x1.fffffffffffffp+1023'


class Simdf64x2ArithmeticCase(Simdf32x4ArithmeticCase):

    LANE_LEN = 2
    LANE_TYPE = 'f64x2'

    floatOp = F64ArithOp()

    FLOAT_NUMBERS = (
        '0x0p+0', '-0x0p+0', '0x1p-1022', '-0x1p-1022', '0x1p-1', '-0x1p-1', '0x1p+0', '-0x1p+0',
        '0x1.921fb54442d18p+2', '-0x1.921fb54442d18p+2', '0x1.fffffffffffffp+1023', '-0x1.fffffffffffffp+1023',
        '0x0.0000000000001p-1022', '0x0.0000000000001p-1022', 'inf', '-inf'
    )
    LITERAL_NUMBERS = ('0123456789', '0123456789e019', '0123456789e+019', '0123456789e-019',
                       '0123456789.', '0123456789.e019', '0123456789.e+019', '0123456789.e-019',
                       '0123456789.0123456789', '0123456789.0123456789e019',
                       '0123456789.0123456789e+019', '0123456789.0123456789e-019',
                       '0x0123456789ABCDEFabcdef', '0x0123456789ABCDEFabcdefp019',
                       '0x0123456789ABCDEFabcdefp+019', '0x0123456789ABCDEFabcdefp-019',
                       '0x0123456789ABCDEFabcdef.', '0x0123456789ABCDEFabcdef.p019',
                       '0x0123456789ABCDEFabcdef.p+019', '0x0123456789ABCDEFabcdef.p-019',
                       '0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdef',
                       '0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp019',
                       '0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp+019',
                       '0x0123456789ABCDEFabcdef.0123456789ABCDEFabcdefp-019'
    )
    NAN_NUMBERS = ('nan', '-nan', 'nan:0x4000000000000', '-nan:0x4000000000000')

    @staticmethod
    def v128_const(lane, value):
        return '(v128.const {lane_type} {value})'.format(lane_type=lane, value=' '.join([str(value)] * 2))

    @property
    def combine_ternary_arith_test_data(self):
        return {
            'add-sub': [
                ['1.125'] * 2, ['0.25'] * 2, ['0.125'] * 2, ['1.0'] * 2
            ],
            'sub-add': [
                ['1.125'] * 2, ['0.25'] * 2, ['0.125'] * 2, ['1.25'] * 2
            ],
            'mul-add': [
                ['1.25'] * 2, ['0.25'] * 2, ['0.25'] * 2, ['0.375'] * 2
            ],
            'mul-sub': [
                ['1.125'] * 2, ['0.125'] * 2, ['0.25'] * 2, ['0.25'] * 2
            ],
            'div-add': [
                ['1.125'] * 2, ['0.125'] * 2, ['0.25'] * 2, ['5.0'] * 2
            ],
            'div-sub': [
                ['1.125'] * 2, ['0.125'] * 2, ['0.25'] * 2, ['4.0'] * 2
            ],
            'mul-div': [
                ['1.125'] * 2, ['0.125'] * 2, ['0.25'] * 2, ['2.25'] * 2
            ],
            'div-mul': [
                ['1.125'] * 2, ['4'] * 2, ['0.25'] * 2, ['18.0'] * 2
            ]
        }

    @property
    def combine_binary_arith_test_data(self):
        return {
            'add-neg': [
                ['1.125'] * 2, ['0.125'] * 2, ['-1.0'] * 2
            ],
            'sub-neg': [
                ['1.125'] * 2, ['0.125'] * 2, ['-1.25'] * 2
            ],
            'mul-neg': [
                ['1.5'] * 2, ['0.25'] * 2, ['-0.375'] * 2
            ],
            'div-neg': [
                ['1.5'] * 2, ['0.25'] * 2, ['-6'] * 2
            ],
            'add-sqrt': [
                ['2.25'] * 2, ['0.25'] * 2, ['1.75'] * 2
            ],
            'sub-sqrt': [
                ['2.25'] * 2, ['0.25'] * 2, ['1.25'] * 2
            ],
            'mul-sqrt': [
                ['2.25'] * 2, ['0.25'] * 2, ['0.375'] * 2
            ],
            'div-sqrt': [
                ['2.25'] * 2, ['0.25'] * 2, ['6'] * 2
            ]
        }

    def get_invalid_cases(self):
        return super().get_invalid_cases().replace('32', '64')

    @property
    def mixed_nan_test_data(self):
        return {
            'neg_canon': [
                ('nan', '1.0'), ('nan:canonical', '-1.0'),
            ],
            'sqrt_canon': [
                ('4.0', '-nan'), ('2.0', 'nan:canonical'),
            ],
            'add_arith': [
                ('nan:0x8000000000000', '1.0'), ('nan', '1.0'),
                ('nan:arithmetic', '2.0'),
            ],
            'sub_arith': [
                ('1.0', '-1.0'), ('-nan', '1.0'), ('nan:canonical', '-2.0'),
            ],
            'mul_mixed': [
                ('nan:0x8000000000000', '1.0'), ('2.0', 'nan'),
                ('nan:arithmetic', 'nan:canonical')
            ],
            'div_mixed': [
                ('nan', '1.0'), ('2.0', '-nan:0x8000000000000'),
                ('nan:canonical', 'nan:arithmetic')
            ]
        }

    def mixed_nan_test(self, cases):
        """Mixed f64x2 tests when only expects NaNs in a subset of lanes."""
        mixed_cases = [
            '\n;; Mixed f64x2 tests when some lanes are NaNs', '(module']
        for test_type, test_data in sorted(self.mixed_nan_test_data.items()):
            op = test_type.split('_')[0]
            if op in self.UNARY_OPS:
                mixed_cases.extend([
                    '  (func (export "{lane}_{t}") (result v128)'.format(lane=self.LANE_TYPE, t=test_type),
                    '    ({lane}.{op} (v128.const {lane} {param})))'.format(
                        lane=self.LANE_TYPE, op=op, param=' '.join(test_data[0]))])
            if op in self.BINARY_OPS:
                mixed_cases.extend([
                    '  (func (export "{lane}_{t}") (result v128)'.format(lane=self.LANE_TYPE, t=test_type),
                    '    ({lane}.{op} (v128.const {lane} {param1}) (v128.const {lane} {param2})))'.format(
                        lane=self.LANE_TYPE, op=op,
                        param1=' '.join(test_data[0]),
                        param2=' '.join(test_data[1]))])
        mixed_cases.append(')\n')
        for test_type, test_data in sorted(self.mixed_nan_test_data.items()):
            mixed_cases.append('(assert_return (invoke "{lane}_{t}") (v128.const {lane} {result}))'.format(
                lane=self.LANE_TYPE, t=test_type, result=' '.join(test_data[-1])
            ))
        cases.extend(mixed_cases)


def gen_test_cases():
    simd_f64x2_arith = Simdf64x2ArithmeticCase()
    simd_f64x2_arith.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()