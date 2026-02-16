#!/usr/bin/env python3

"""
Generate f32x4 [pmin, pmax] cases.
"""

from simd_f32x4_arith import Simdf32x4ArithmeticCase
from simd_float_op import FloatingPointSimpleOp
from simd import SIMD
from test_assert import AssertReturn


class Simdf32x4PminPmaxCase(Simdf32x4ArithmeticCase):
    UNARY_OPS = ()
    BINARY_OPS = ('pmin', 'pmax',)
    floatOp = FloatingPointSimpleOp()

    def get_combine_cases(self):
        return ''

    def get_normal_case(self):
        """Normal test cases from WebAssembly core tests.
        """
        cases = []
        binary_test_data = []
        unary_test_data = []

        for op in self.BINARY_OPS:
            op_name = self.full_op_name(op)
            for operand1 in self.FLOAT_NUMBERS + self.LITERAL_NUMBERS:
                for operand2 in self.FLOAT_NUMBERS + self.LITERAL_NUMBERS:
                    result = self.floatOp.binary_op(op, operand1, operand2)
                    binary_test_data.append([op_name, operand1, operand2, result])

            # pmin and pmax always return operand1 if either operand is a nan
            for operand1 in self.NAN_NUMBERS:
                for operand2 in self.FLOAT_NUMBERS + self.LITERAL_NUMBERS + self.NAN_NUMBERS:
                    binary_test_data.append([op_name, operand1, operand2, operand1])
            for operand2 in self.NAN_NUMBERS:
                for operand1 in self.FLOAT_NUMBERS + self.LITERAL_NUMBERS:
                    binary_test_data.append([op_name, operand1, operand2, operand1])

        for case in binary_test_data:
            cases.append(str(AssertReturn(case[0],
                        [SIMD.v128_const(c, self.LANE_TYPE) for c in case[1:-1]],
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

            for op in self.BINARY_OPS:
                cases.append(tpl_assert.format(lane_type=lane_type, op=op, value=' '.join([self.v128_const('i32x4', '0')]*2)))

    def gen_test_cases(self):
        wast_filename = '../simd_{lane_type}_pmin_pmax.wast'.format(lane_type=self.LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            txt_test_case = self.get_all_cases()
            txt_test_case = txt_test_case.replace(
                    self.LANE_TYPE + ' arithmetic',
                    self.LANE_TYPE + ' [pmin, pmax]')
            fp.write(txt_test_case)


def gen_test_cases():
    simd_f32x4_pmin_pmax_case = Simdf32x4PminPmaxCase()
    simd_f32x4_pmin_pmax_case.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
