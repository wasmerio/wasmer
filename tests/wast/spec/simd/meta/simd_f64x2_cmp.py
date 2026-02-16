#!/usr/bin/env python3

"""
This file is used for generating simd_f64x2_cmp.wast file.
Which inherites from `SimdArithmeticCase` class, overloads
the `get_test_cases` method, and reset the Test Case template.
The reason why this is different from other cmp files is that
f64x2 only has 6 comparison instructions but with amounts of
test datas.
"""

from simd_arithmetic import SimdArithmeticCase
from simd_float_op import FloatingPointCmpOp
from test_assert import AssertReturn
from simd import SIMD


class Simdf64x2CmpCase(SimdArithmeticCase):
    LANE_LEN = 4
    LANE_TYPE = 'f64x2'

    UNARY_OPS = ()
    BINARY_OPS = ('eq', 'ne', 'lt', 'le', 'gt', 'ge',)
    floatOp = FloatingPointCmpOp()

    FLOAT_NUMBERS_SPECIAL = ('0x1p-1074', '-inf', '0x1.921fb54442d18p+2',
                             '0x1p+0', '-0x1.fffffffffffffp+1023', '-0x0p+0', '-0x1p-1', '0x1.fffffffffffffp+1023',
                             '-0x1p-1074', '-0x1p-1022', '0x1p-1', '-0x1.921fb54442d18p+2',
                             '0x0p+0', 'inf', '-0x1p+0', '0x1p-1022'
                            )
    LITERAL_NUMBERS = ('01234567890123456789e038', '01234567890123456789e-038',
                       '0123456789.e038', '0123456789.e+038',
                       '01234567890123456789.01234567890123456789'

    )
    FLOAT_NUMBERS_NORMAL = ('-1', '0', '1', '2.0')
    NAN_NUMBERS = ('nan', '-nan', 'nan:0x4000000000000', '-nan:0x4000000000000')

    def full_op_name(self, op_name):
        return self.LANE_TYPE + '.' + op_name

    @staticmethod
    def v128_const(lane, value):
        lane_cnt = 2 if lane in ['f64x2', 'i64x2'] else 4
        return '(v128.const {lane_type} {value})'.format(lane_type=lane, value=' '.join([str(value)] * lane_cnt))

    @property
    def combine_ternary_arith_test_data(self):
        return {}

    @property
    def combine_binary_arith_test_data(self):
        return ['f64x2.eq', 'f64x2.ne', 'f64x2.lt', 'f64x2.le', 'f64x2.gt', 'f64x2.ge']

    def get_combine_cases(self):
        combine_cases = [';; combination\n(module (memory 1)']

        # append funcs
        binary_func_template = '  (func (export "{op}-in-block")\n' \
                             '    (block\n' \
                             '      (drop\n' \
                             '        (block (result v128)\n' \
                             '          ({op}\n' \
                             '            (block (result v128) (v128.load (i32.const 0)))\n' \
                             '            (block (result v128) (v128.load (i32.const 1)))\n' \
                             '          )\n' \
                             '        )\n' \
                             '      )\n' \
                             '    )\n' \
                             '  )'
        for func in self.combine_binary_arith_test_data:
            combine_cases.append(binary_func_template.format(op=func))

        binary_func_template = '  (func (export "nested-{func}")\n' \
                             '    (drop\n' \
                             '      ({func}\n' \
                             '        ({func}\n' \
                             '          ({func}\n' \
                             '            (v128.load (i32.const 0))\n' \
                             '            (v128.load (i32.const 1))\n' \
                             '          )\n' \
                             '          ({func}\n' \
                             '            (v128.load (i32.const 2))\n' \
                             '            (v128.load (i32.const 3))\n' \
                             '          )\n' \
                             '        )\n' \
                             '        ({func}\n' \
                             '          ({func}\n' \
                             '            (v128.load (i32.const 0))\n' \
                             '            (v128.load (i32.const 1))\n' \
                             '          )\n' \
                             '          ({func}\n' \
                             '            (v128.load (i32.const 2))\n' \
                             '            (v128.load (i32.const 3))\n' \
                             '          )\n' \
                             '        )\n' \
                             '      )\n' \
                             '    )\n' \
                             '  )' \

        for func in self.combine_binary_arith_test_data:
            combine_cases.append(binary_func_template.format(func=func))

        combine_cases.append('  (func (export "as-param")\n'
                             '    (drop\n'
                             '      (f64x2.eq\n'
                             '        (f64x2.ne\n'
                             '          (f64x2.lt\n'
                             '            (v128.load (i32.const 0))\n'
                             '            (v128.load (i32.const 1))\n'
                             '          )\n'
                             '          (f64x2.le\n'
                             '            (v128.load (i32.const 2))\n'
                             '            (v128.load (i32.const 3))\n'
                             '          )\n'
                             '        )\n'
                             '        (f64x2.gt\n'
                             '          (f64x2.ge\n'
                             '            (v128.load (i32.const 0))\n'
                             '            (v128.load (i32.const 1))\n'
                             '          )\n'
                             '          (f64x2.eq\n'
                             '            (v128.load (i32.const 2))\n'
                             '            (v128.load (i32.const 3))\n'
                             '          )\n'
                             '        )\n'
                             '      )\n'
                             '    )\n'
                             '  )')

        combine_cases.append(')')

        # append assert
        binary_case_template = ('(assert_return (invoke "{func}-in-block"))')
        for func in self.combine_binary_arith_test_data:
            combine_cases.append(binary_case_template.format(func=func))

        binary_case_template = ('(assert_return (invoke "nested-{func}"))')
        for func in self.combine_binary_arith_test_data:
            combine_cases.append(binary_case_template.format(func=func))

        combine_cases.append('(assert_return (invoke "as-param"))\n')

        return '\n'.join(combine_cases)

    def get_normal_case(self):
        """Normal test cases from WebAssembly core tests
        """
        cases = []
        binary_test_data = []
        unary_test_data = []

        for op in self.BINARY_OPS:
            op_name = self.full_op_name(op)
            for operand1 in self.FLOAT_NUMBERS_SPECIAL:
                for operand2 in self.FLOAT_NUMBERS_SPECIAL + self.NAN_NUMBERS:
                    result = self.floatOp.binary_op(op, operand1, operand2)
                    binary_test_data.append([op_name, operand1, operand2, result])

            for operand1 in self.LITERAL_NUMBERS:
                for operand2 in self.LITERAL_NUMBERS:
                    result = self.floatOp.binary_op(op, operand1, operand2)
                    binary_test_data.append([op_name, operand1, operand2, result])

            for operand1 in self.NAN_NUMBERS:
                for operand2 in self.FLOAT_NUMBERS_SPECIAL + self.NAN_NUMBERS:
                    result = self.floatOp.binary_op(op, operand1, operand2)
                    binary_test_data.append([op_name, operand1, operand2, result])

        for op in self.BINARY_OPS:
            op_name = self.full_op_name(op)
            for operand1 in self.FLOAT_NUMBERS_NORMAL:
                for operand2 in self.FLOAT_NUMBERS_NORMAL:
                    result = self.floatOp.binary_op(op, operand1, operand2)
                    binary_test_data.append([op_name, operand1, operand2, result])

        for case in binary_test_data:
            cases.append(str(AssertReturn(case[0],
                        [SIMD.v128_const(elem, self.LANE_TYPE) for elem in case[1:-1]],
                        SIMD.v128_const(case[-1], 'i64x2'))))

        for case in unary_test_data:
            cases.append(str(AssertReturn(case[0],
                        [SIMD.v128_const(elem, self.LANE_TYPE) for elem in case[1:-1]],
                        SIMD.v128_const(case[-1], 'i64x2'))))

        self.get_unknown_operator_case(cases)

        return '\n'.join(cases)

    def get_unknown_operator_case(self, cases):
        """Unknown operator cases.
        """

        tpl_assert = "(assert_malformed (module quote \"(memory 1) (func " \
                     " (param $x v128) (param $y v128) (result v128) " \
                     "({lane_type}.{op} (local.get $x) (local.get $y)))\") \"unknown operator\")"

        cases.append('\n\n;; unknown operators')

        for lane_type in ['f2x64']:
            for op in self.BINARY_OPS:
                cases.append(tpl_assert.format(lane_type=lane_type,
                                               op=op))

    def gen_test_cases(self):
        wast_filename = '../simd_{lane_type}_cmp.wast'.format(lane_type=self.LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            txt_test_case = self.get_all_cases()
            txt_test_case = txt_test_case.replace('f64x2 arithmetic', 'f64x2 comparison')
            fp.write(txt_test_case)


def gen_test_cases():
    simd_f64x2_cmp = Simdf64x2CmpCase()
    simd_f64x2_cmp.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()