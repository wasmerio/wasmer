#!/usr/bin/env python3

"""
Generate f32x4 floating-point arithmetic operation cases.
"""

from simd_arithmetic import SimdArithmeticCase
from simd_float_op import FloatingPointArithOp
from test_assert import AssertReturn
from simd import SIMD


class F32ArithOp(FloatingPointArithOp):
    maximum = '0x1.fffffep+127'


class Simdf32x4ArithmeticCase(SimdArithmeticCase):
    LANE_LEN = 4
    LANE_TYPE = 'f32x4'

    floatOp = F32ArithOp()
    UNARY_OPS = ('neg', 'sqrt')
    BINARY_OPS = ('add', 'sub', 'mul', 'div')

    FLOAT_NUMBERS = (
        '0x0p+0', '-0x0p+0', '0x1p-149', '-0x1p-149', '0x1p-126', '-0x1p-126', '0x1p-1', '-0x1p-1', '0x1p+0', '-0x1p+0',
        '0x1.921fb6p+2', '-0x1.921fb6p+2', '0x1.fffffep+127', '-0x1.fffffep+127', 'inf', '-inf'
    )
    LITERAL_NUMBERS = ('0123456789', '0123456789e019', '0123456789e+019', '0123456789e-019',
                       '0123456789.', '0123456789.e019', '0123456789.e+019', '0123456789.e-019',
                       '0123456789.0123456789', '0123456789.0123456789e019',
                       '0123456789.0123456789e+019', '0123456789.0123456789e-019',
                       '0x0123456789ABCDEF', '0x0123456789ABCDEFp019',
                       '0x0123456789ABCDEFp+019', '0x0123456789ABCDEFp-019',
                       '0x0123456789ABCDEF.', '0x0123456789ABCDEF.p019',
                       '0x0123456789ABCDEF.p+019', '0x0123456789ABCDEF.p-019',
                       '0x0123456789ABCDEF.019aF', '0x0123456789ABCDEF.019aFp019',
                       '0x0123456789ABCDEF.019aFp+019', '0x0123456789ABCDEF.019aFp-019'
    )
    NAN_NUMBERS = ('nan', '-nan', 'nan:0x200000', '-nan:0x200000')

    def full_op_name(self, op_name):
        return self.LANE_TYPE + '.' + op_name

    @staticmethod
    def v128_const(lane, value):
        return '(v128.const {lane_type} {value})'.format(lane_type=lane, value=' '.join([str(value)] * 4))

    @property
    def combine_ternary_arith_test_data(self):
        return {
            'add-sub': [
                ['1.125'] * 4, ['0.25'] * 4, ['0.125'] * 4, ['1.0'] * 4
            ],
            'sub-add': [
                ['1.125'] * 4, ['0.25'] * 4, ['0.125'] * 4, ['1.25'] * 4
            ],
            'mul-add': [
                ['1.25'] * 4, ['0.25'] * 4, ['0.25'] * 4, ['0.375'] * 4
            ],
            'mul-sub': [
                ['1.125'] * 4, ['0.125'] * 4, ['0.25'] * 4, ['0.25'] * 4
            ],
            'div-add': [
                ['1.125'] * 4, ['0.125'] * 4, ['0.25'] * 4, ['5.0'] * 4
            ],
            'div-sub': [
                ['1.125'] * 4, ['0.125'] * 4, ['0.25'] * 4, ['4.0'] * 4
            ],
            'mul-div': [
                ['1.125'] * 4, ['0.125'] * 4, ['0.25'] * 4, ['2.25'] * 4
            ],
            'div-mul': [
                ['1.125'] * 4, ['4'] * 4, ['0.25'] * 4, ['18.0'] * 4
            ]
        }

    @property
    def combine_binary_arith_test_data(self):
        return {
            'add-neg': [
                ['1.125'] * 4, ['0.125'] * 4, ['-1.0'] * 4
            ],
            'sub-neg': [
                ['1.125'] * 4, ['0.125'] * 4, ['-1.25'] * 4
            ],
            'mul-neg': [
                ['1.5'] * 4, ['0.25'] * 4, ['-0.375'] * 4
            ],
            'div-neg': [
                ['1.5'] * 4, ['0.25'] * 4, ['-6'] * 4
            ],
            'add-sqrt': [
                ['2.25'] * 4, ['0.25'] * 4, ['1.75'] * 4
            ],
            'sub-sqrt': [
                ['2.25'] * 4, ['0.25'] * 4, ['1.25'] * 4
            ],
            'mul-sqrt': [
                ['2.25'] * 4, ['0.25'] * 4, ['0.375'] * 4
            ],
            'div-sqrt': [
                ['2.25'] * 4, ['0.25'] * 4, ['6'] * 4
            ]
        }

    def get_normal_case(self):
        """Normal test cases from WebAssembly core tests
        """
        cases = []
        binary_test_data = []
        unary_test_data = []

        for op in self.BINARY_OPS:
            op_name = self.full_op_name(op)
            for operand1 in self.FLOAT_NUMBERS:
                for operand2 in self.FLOAT_NUMBERS:
                    result = self.floatOp.binary_op(op, operand1, operand2)
                    if 'nan' not in result:
                        # Normal floating point numbers as the results
                        binary_test_data.append([op_name, operand1, operand2, result])
                    else:
                        # Since the results contain the 'nan' string, the result literals would be
                        # nan:canonical
                        binary_test_data.append([op_name, operand1, operand2, 'nan:canonical'])

            for operand1 in self.NAN_NUMBERS:
                for operand2 in self.FLOAT_NUMBERS:
                    if 'nan:' in operand1 or 'nan:' in operand2:
                        # When the arguments contain 'nan:', the result literal is nan:arithmetic
                        # Consider the different order of arguments as different cases.
                        binary_test_data.append([op_name, operand1, operand2, 'nan:arithmetic'])
                        binary_test_data.append([op_name, operand2, operand1, 'nan:arithmetic'])
                    else:
                        # No 'nan' string found, then the result literal is nan:canonical.
                        binary_test_data.append([op_name, operand1, operand2, 'nan:canonical'])
                        binary_test_data.append([op_name, operand2, operand1, 'nan:canonical'])
                for operand2 in self.NAN_NUMBERS:
                    if 'nan:' in operand1 or 'nan:' in operand2:
                        binary_test_data.append([op_name, operand1, operand2, 'nan:arithmetic'])
                    else:
                        binary_test_data.append([op_name, operand1, operand2, 'nan:canonical'])

            for operand in self.LITERAL_NUMBERS:
                if self.LANE_TYPE == 'f32x4':
                    single_precision = True
                else:
                    single_precision = False
                result = self.floatOp.binary_op(op, operand, operand, single_prec=single_precision)
                binary_test_data.append([op_name, operand, operand, result])

        for case in binary_test_data:
            cases.append(str(AssertReturn(case[0],
                        [SIMD.v128_const(elem, self.LANE_TYPE) for elem in case[1:-1]],
                        SIMD.v128_const(case[-1], self.LANE_TYPE))))

        for operand in self.FLOAT_NUMBERS + self.NAN_NUMBERS + self.LITERAL_NUMBERS:
            if 'nan:' in operand:
                unary_test_data.append([op_name, operand, 'nan:arithmetic'])
            elif 'nan' in operand:
                unary_test_data.append([op_name, operand, 'nan:canonical'])
            else:
                # Normal floating point numbers for sqrt operation
                op_name = self.full_op_name('sqrt')
                result = self.floatOp.float_sqrt(operand)
                if 'nan' not in result:
                    # Get the sqrt value correctly
                    unary_test_data.append([op_name, operand, result])
                else:
                    #
                    unary_test_data.append([op_name, operand, 'nan:canonical'])

        for operand in self.FLOAT_NUMBERS + self.NAN_NUMBERS + self.LITERAL_NUMBERS:
            op_name = self.full_op_name('neg')
            result = self.floatOp.float_neg(operand)
            # Neg operation is valid for all the floating point numbers
            unary_test_data.append([op_name, operand, result])

        for case in unary_test_data:
            cases.append(str(AssertReturn(case[0],
                        [SIMD.v128_const(elem, self.LANE_TYPE) for elem in case[1:-1]],
                        SIMD.v128_const(case[-1], self.LANE_TYPE))))

        self.mixed_nan_test(cases)

        return '\n'.join(cases)

    @property
    def mixed_sqrt_nan_test_data(self):
        return {
            "sqrt_canon": [
                ('-1.0', 'nan', '4.0', '9.0'),
                ('nan:canonical', 'nan:canonical', '2.0', '3.0')
            ],
            'sqrt_arith': [
                ('nan:0x200000', '-nan:0x200000', '16.0', '25.0'),
                ('nan:arithmetic', 'nan:arithmetic', '4.0', '5.0')
            ],
            'sqrt_mixed': [
                ('-inf', 'nan:0x200000', '36.0', '49.0'),
                ('nan:canonical', 'nan:arithmetic', '6.0', '7.0')
            ]
        }

    def mixed_nan_test(self, cases):
        """Mixed f32x4 tests when only expects NaNs in a subset of lanes.
        """
        mixed_cases = ['\n\n;; Mixed f32x4 tests when some lanes are NaNs', '(module\n']
        cases.extend(mixed_cases)
        for test_type, test_data in sorted(self.mixed_sqrt_nan_test_data.items()):
            func = ['  (func (export "{lane}_{t}") (result v128)'.format(
                lane=self.LANE_TYPE, t=test_type),
                    '    ({lane}.{op} (v128.const {lane} {value})))'.format(
                lane=self.LANE_TYPE, op=test_type.split('_')[0], value=' '.join(test_data[0]))]
            cases.extend(func)
        cases.append(')\n')

        for test_type, test_data in sorted(self.mixed_sqrt_nan_test_data.items()):
            cases.append('(assert_return (invoke "{lane}_{t}") (v128.const {lane} {result}))'.format(
                lane=self.LANE_TYPE, t=test_type, result=' '.join(test_data[1])))


def gen_test_cases():
    simd_f32x4_arith = Simdf32x4ArithmeticCase()
    simd_f32x4_arith.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()