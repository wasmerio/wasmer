#!/usr/bin/env python3

"""
Generate f64x2 [abs, min, max] cases.
"""

from simd_f32x4 import Simdf32x4Case
from simd_f32x4_arith import Simdf32x4ArithmeticCase
from test_assert import AssertReturn
from simd import SIMD


class Simdf64x2Case(Simdf32x4Case):

    LANE_TYPE = 'f64x2'

    FLOAT_NUMBERS = (
        '0x0p+0', '-0x0p+0', '0x1p-1074', '-0x1p-1074', '0x1p-1022', '-0x1p-1022', '0x1p-1', '-0x1p-1', '0x1p+0', '-0x1p+0',
        '0x1.921fb54442d18p+2', '-0x1.921fb54442d18p+2', '0x1.fffffffffffffp+1023', '-0x1.fffffffffffffp+1023', 'inf', '-inf'
    )
    LITERAL_NUMBERS = ('01234567890123456789e038', '01234567890123456789e-038',
                       '0123456789.e038', '0123456789.e+038',
                       '-01234567890123456789.01234567890123456789'
    )
    NAN_NUMBERS = ('nan', '-nan', 'nan:0x4000000000000', '-nan:0x4000000000000')

    def gen_test_func_template(self):

        # Get function code
        template = Simdf32x4ArithmeticCase.gen_test_func_template(self)

        # Function template
        tpl_func = '  (func (export "{func}"){params} (result v128) ({op} {operand_1}{operand_2}))'

        # Raw data list specific for "const vs const" and "param vs const" tests"
        const_test_raw_data = [
            [
                [['0', '1'], ['0', '2']],
                [['0', '1'], ['0', '2']]
            ],
            [
                [['2', '-3'], ['1', '3']],
                [['1', '-3'], ['2', '3']]
            ],
            [
                [['0', '1'], ['0', '1']],
                [['0', '1'], ['0', '1']]
            ],
            [
                [['2', '3'], ['2', '3']],
                [['2', '3'], ['2', '3']]
            ],
            [
                [['0x00', '0x01'], ['0x00', '0x02']],
                [['0x00', '0x01'], ['0x00', '0x02']]
            ],
            [
                [['0x02', '0x80000000'], ['0x01', '2147483648']],
                [['0x01', '0x80000000'], ['0x02', '2147483648']]
            ],
            [
                [['0x00', '0x01'], ['0x00', '0x01']],
                [['0x00', '0x01'], ['0x00', '0x01']]
            ],
            [
                [['0x02', '0x80000000'], ['0x02', '0x80000000']],
                [['0x02', '0x80000000'], ['0x02', '0x80000000']]
            ]
        ]

        # Test data list combined with `const_test_raw_data` and corresponding ops and function names
        # specific for "const vs const" and "param vs const" tests
        const_test_data = {}

        # Generate func and assert
        for op in self.BINARY_OPS:

            op_name = self.full_op_name(op)

            # Add comment for the case script "  ;; [f64x2.min, f64x2.max] const vs const"
            template.insert(len(template)-1, '  ;; {} const vs const'.format(op_name))

            # Add const vs const cases
            for case_data in const_test_raw_data:

                func = "{op}_with_const_{index}".format(op=op_name, index=len(template)-7)
                template.insert(len(template)-1,
                                tpl_func.format(func=func, params='', op=op_name,
                                                operand_1=self.v128_const('f64x2', case_data[0][0]),
                                                operand_2=' ' + self.v128_const('f64x2', case_data[0][1])))

                ret_idx = 0 if op == 'min' else 1

                if op not in const_test_data:
                    const_test_data[op] = []

                const_test_data[op].append([func, case_data[1][ret_idx]])

            # Add comment for the case script "  ;; [f64x2.min, f64x2.max] param vs const"
            template.insert(len(template)-1, '  ;; {} param vs const'.format(op_name))

            case_cnt = 0

            # Add param vs const cases
            for case_data in const_test_raw_data:

                func = "{op}_with_const_{index}".format(op=op_name, index=len(template)-7)

                # Cross parameters and constants
                if case_cnt in (0, 3):
                    operand_1 = '(local.get 0)'
                    operand_2 = self.v128_const('f64x2', case_data[0][0])
                else:
                    operand_1 = self.v128_const('f64x2', case_data[0][0])
                    operand_2 = '(local.get 0)'

                template.insert(len(template)-1,
                                tpl_func.format(func=func, params=' (param v128)', op=op_name,
                                                operand_1=operand_1, operand_2=' ' + operand_2))

                ret_idx = 0 if op == 'min' else 1

                if op not in const_test_data:
                    const_test_data[op] = []

                const_test_data[op].append([func, case_data[0][1], case_data[1][ret_idx]])

                case_cnt += 1

        # Generate func for abs
        op_name = self.full_op_name('abs')
        template.insert(len(template)-1, '')
        func = "{op}_with_const_{index}".format(op=op_name, index=35)
        template.insert(len(template)-1,
                        tpl_func.format(func=func, params='', op=op_name,
                                        operand_1=self.v128_const('f64x2', ['-0', '-1']), operand_2=''))
        func = "{op}_with_const_{index}".format(op=op_name, index=36)
        template.insert(len(template)-1,
                        tpl_func.format(func=func, params='', op=op_name,
                                        operand_1=self.v128_const('f64x2', ['-2', '-3']), operand_2=''))

        # Test different lanes go through different if-then clauses
        lst_diff_lane_vs_clause = [
            [
                'f64x2.min',
                [['nan', '0'], ['0', '1']],
                [['nan:canonical', '0']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.min',
                [['0', '1'], ['-nan', '0']],
                [['nan:canonical', '0']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.min',
                [['0', '1'], ['-nan', '1']],
                [['nan:canonical', '1']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.max',
                [['nan', '0'], ['0', '1']],
                [['nan:canonical', '1']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.max',
                [['0', '1'], ['-nan', '0']],
                [['nan:canonical', '1']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.max',
                [['0', '1'], ['-nan', '1']],
                [['nan:canonical', '1']],
                ['f64x2', 'f64x2', 'f64x2']
            ]
        ]

        # Template for assert
        tpl_assert = '(assert_return\n' \
                     '  (invoke "{func}"\n' \
                     '    {operand_1}\n' \
                     '    {operand_2}\n' \
                     '  )\n' \
                     '  {expected_result}\n' \
                     ')'

        lst_diff_lane_vs_clause_assert = []

        # Add comment in wast script
        lst_diff_lane_vs_clause_assert.append('')
        lst_diff_lane_vs_clause_assert.append(';; Test different lanes go through different if-then clauses')

        for case_data in lst_diff_lane_vs_clause:

            lst_diff_lane_vs_clause_assert.append(';; {lane_type}'.format(lane_type=case_data[0]))

            lst_diff_lane_vs_clause_assert.append(tpl_assert.format(
                func=case_data[0],
                operand_1=self.v128_const(case_data[3][0], case_data[1][0]),
                operand_2=self.v128_const(case_data[3][1], case_data[1][1]),
                expected_result=self.v128_const(case_data[3][2], case_data[2][0])
            ))

        lst_diff_lane_vs_clause_assert.append('')

        # Add test for operations with constant operands
        for key in const_test_data:
            op_name = self.full_op_name(key)
            case_cnt = 0
            for case_data in const_test_data[key]:

                # Add comment for the param combination
                if case_cnt == 0:
                    template.append(';; {} const vs const'.format(op_name))
                if case_cnt == 4:
                    template.append(';; {} param vs const'.format(op_name))

                # Cross parameters and constants
                if case_cnt < 8:
                    template.append(str(AssertReturn(case_data[0], [], self.v128_const('f64x2', case_data[1]))))
                else:
                    template.append(str(AssertReturn(case_data[0], [self.v128_const('f64x2', case_data[1])], self.v128_const('f64x2', case_data[2]))))
                case_cnt += 1

        # Generate and append f64x2.abs assert
        op_name = self.full_op_name('abs')
        template.append('')
        func = "{op}_with_const_{index}".format(op=op_name, index=35)
        template.append(str(AssertReturn(func, [], self.v128_const('f64x2', ['0', '1']))))
        func = "{op}_with_const_{index}".format(op=op_name, index=36)
        template.append(str(AssertReturn(func, [], self.v128_const('f64x2', ['2', '3']))))

        template.extend(lst_diff_lane_vs_clause_assert)

        return template

    @property
    def combine_ternary_arith_test_data(self):
        # This method overrides the base class method from SimdArithmeticCase
        # used for generating test data for min and max combination tests.
        return {
            'min-max': [
                ['1.125'] * 2, ['0.25'] * 2, ['0.125'] * 2, ['0.125'] * 2
            ],
            'max-min': [
                ['1.125'] * 2, ['0.25'] * 2, ['0.125'] * 2, ['0.25'] * 2
            ]
        }

    @property
    def combine_binary_arith_test_data(self):
        # This method overrides the base class method from SimdArithmeticCase
        # used for generating test data for min, max and abs combination tests.
        return {
            'min-abs': [
                ['-1.125'] * 2, ['0.125'] * 2, ['0.125'] * 2
            ],
            'max-abs': [
                ['-1.125'] * 2, ['0.125'] * 2, ['1.125'] * 2
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
                        binary_test_data.append([op_name, operand1, operand2, 'nan:arithmetic'])
                    else:
                        # No 'nan' string found, then the result literal is nan:canonical
                        binary_test_data.append([op_name, operand1, operand2, 'nan:canonical'])
                for operand2 in self.NAN_NUMBERS:
                    if 'nan:' in operand1 or 'nan:' in operand2:
                        binary_test_data.append([op_name, operand1, operand2, 'nan:arithmetic'])
                    else:
                        binary_test_data.append([op_name, operand1, operand2, 'nan:canonical'])

            for operand1 in self.LITERAL_NUMBERS:
                for operand2 in self.LITERAL_NUMBERS:
                    result = self.floatOp.binary_op(op, operand1, operand2, hex_form=False)
                    binary_test_data.append([op_name, operand1, operand2, result])

        for case in binary_test_data:
            cases.append(str(AssertReturn(case[0],
                        [SIMD.v128_const(elem, self.LANE_TYPE) for elem in case[1:-1]],
                        SIMD.v128_const(case[-1], self.LANE_TYPE))))

        # Test opposite signs of zero
        lst_oppo_signs_0 = [
            '\n;; Test opposite signs of zero',
            [
                'f64x2.min',
                [['0', '0'], ['+0', '-0']],
                [['0', '-0']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.min',
                [['-0', '+0'], ['+0', '-0']],
                [['-0', '-0']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.min',
                [['-0', '-0'], ['+0', '+0']],
                [['-0', '-0']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.max',
                [['0', '0'], ['+0', '-0']],
                [['0', '0']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.max',
                [['-0', '+0'], ['+0', '-0']],
                [['0', '0']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            [
                'f64x2.max',
                [['-0', '-0'], ['+0', '+0']],
                [['+0', '+0']],
                ['f64x2', 'f64x2', 'f64x2']
            ],
            '\n'
        ]

        # Generate test case for opposite signs of zero
        for case_data in lst_oppo_signs_0:

            if isinstance(case_data, str):
                cases.append(case_data)
                continue

            cases.append(str(AssertReturn(case_data[0],
                                          [self.v128_const(case_data[3][0], case_data[1][0]),
                                           self.v128_const(case_data[3][1], case_data[1][1])],
                                          self.v128_const(case_data[3][2], case_data[2][0]))))

        for p in self.FLOAT_NUMBERS + self.LITERAL_NUMBERS:
            op_name = self.full_op_name('abs')
            hex_literal = True
            if p in self.LITERAL_NUMBERS:
                hex_literal = False
            result = self.floatOp.unary_op('abs', p, hex_form=hex_literal)
            # Abs operation is valid for all the floating point numbers
            unary_test_data.append([ op_name, p, result])

        for case in unary_test_data:
            cases.append(str(AssertReturn(case[0],
                        [SIMD.v128_const(c, self.LANE_TYPE) for c in case[1:-1]],
                        SIMD.v128_const(case[-1], self.LANE_TYPE))))

        return '\n'.join(cases)

    def gen_test_cases(self):
        wast_filename = '../simd_{lane_type}.wast'.format(lane_type=self.LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            txt_test_case = self.get_all_cases()
            txt_test_case = txt_test_case.replace('f64x2 arithmetic', 'f64x2 [abs, min, max]')
            fp.write(txt_test_case)


def gen_test_cases():
    simd_f64x2_case = Simdf64x2Case()
    simd_f64x2_case.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()