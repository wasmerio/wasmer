#!/usr/bin/env python3

"""
Generate f32x4 [ceil, floor, trunc, nearest] cases.
"""

from simd_f32x4_arith import Simdf32x4ArithmeticCase
from simd_float_op import FloatingPointRoundingOp
from simd import SIMD
from test_assert import AssertReturn


class Simdf32x4RoundingCase(Simdf32x4ArithmeticCase):
    UNARY_OPS = ('ceil', 'floor', 'trunc', 'nearest')
    BINARY_OPS = ()
    floatOp = FloatingPointRoundingOp()

    def get_combine_cases(self):
        return ''

    def get_normal_case(self):
        """Normal test cases from WebAssembly core tests.
        """
        cases = []
        unary_test_data = []

        for op in self.UNARY_OPS:
            op_name = self.full_op_name(op)
            for operand in self.FLOAT_NUMBERS:
                result = self.floatOp.unary_op(op, operand)
                if 'nan' in result:
                    unary_test_data.append([op_name, operand, 'nan:canonical'])
                else:
                    unary_test_data.append([op_name, operand, result])

            for operand in self.LITERAL_NUMBERS:
                result = self.floatOp.unary_op(op, operand, hex_form=False)
                unary_test_data.append([op_name, operand, result])

            for operand in self.NAN_NUMBERS:
                if 'nan:' in operand:
                    unary_test_data.append([op_name, operand, 'nan:arithmetic'])
                else:
                    unary_test_data.append([op_name, operand, 'nan:canonical'])

        for case in unary_test_data:
            cases.append(str(AssertReturn(case[0],
                        [SIMD.v128_const(elem, self.LANE_TYPE) for elem in case[1:-1]],
                        SIMD.v128_const(case[-1], self.LANE_TYPE))))

        self.get_unknown_operator_case(cases)

        return '\n'.join(cases)

    def get_unknown_operator_case(self, cases):
        """Unknown operator cases.
        """

        tpl_assert = "(assert_malformed (module quote \"(memory 1) (func (result v128) " \
                     "({lane_type}.{op} {value}))\") \"unknown operator\")"

        unknown_op_cases = ['\n\n;; Unknown operators\n']
        cases.extend(unknown_op_cases)

        for lane_type in ['i8x16', 'i16x8', 'i32x4', 'i64x2']:
            for op in self.UNARY_OPS:
                cases.append(tpl_assert.format(lane_type=lane_type, op=op, value=self.v128_const('i32x4', '0')))

    def gen_test_cases(self):
        wast_filename = '../simd_{lane_type}_rounding.wast'.format(lane_type=self.LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            txt_test_case = self.get_all_cases()
            txt_test_case = txt_test_case.replace(
                    self.LANE_TYPE + ' arithmetic',
                    self.LANE_TYPE + ' [ceil, floor, trunc, nearest]')
            fp.write(txt_test_case)


def gen_test_cases():
    simd_f32x4_case = Simdf32x4RoundingCase()
    simd_f32x4_case.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
