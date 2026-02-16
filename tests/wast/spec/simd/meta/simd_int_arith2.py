#!/usr/bin/env python3

"""
Generate [min_s, min_u, max_s, max_u] cases for i32x4, i16x8 and i8x16.
"""

from simd import SIMD
from test_assert import AssertReturn, AssertInvalid, AssertMalformed
from simd_lane_value import LaneValue
from simd_integer_op import ArithmeticOp


class SimdLaneWiseInteger:
    LANE_TYPE = None

    LANE_VALUE = None

    BINARY_OPS = ('min_s', 'min_u', 'max_s', 'max_u',)

    UNARY_OPS = ('abs',)

    class_summary = """;; Tests for {lane_type} [min_s, min_u, max_s, max_u, avgr_u, abs] operations."""

    def __init__(self):

        self.LANE_VALUE = LaneValue(self.lane_width)

    @property
    def lane_count(self):
        """count of lanes"""
        return int(self.LANE_TYPE.split('x')[1])

    @property
    def lane_width(self):
        """width of a single lane"""
        return int(self.LANE_TYPE.replace('i', '').split('x')[0])

    @property
    def get_unary_complex_test_data(self):
        """test const vs const and different lanes go through different if-then clauses for unary ops"""
        case_data = [
            [self.LANE_VALUE.min, self.LANE_VALUE.max, self.LANE_VALUE.quarter, self.LANE_VALUE.mask]
        ]
        case_data = [list(map(str, param)) for param in case_data]

        return case_data

    @property
    def get_binary_test_data_with_const(self):
        """test const vs const and param vs const for binary ops"""
        case_data = [
            [
                [self.LANE_VALUE.min, self.LANE_VALUE.max, self.LANE_VALUE.quarter, self.LANE_VALUE.mask],
                [self.LANE_VALUE.mask, self.LANE_VALUE.quarter, self.LANE_VALUE.max, self.LANE_VALUE.min]
            ],
            [
                [0, 1, 2, 3],
                [3, 2, 1, 0],
            ]
        ]
        case_data = [[list(map(str, param_1)), list(map(str, param_2))] for param_1, param_2 in case_data]

        return case_data

    @property
    def get_binary_test_data_go_through_if(self):
        """test different lanes go through different if-then clauses"""
        case_data = [
            [
                [self.LANE_VALUE.min, self.LANE_VALUE.max, self.LANE_VALUE.quarter, self.LANE_VALUE.mask],
                [self.LANE_VALUE.mask, self.LANE_VALUE.quarter, self.LANE_VALUE.max, self.LANE_VALUE.min]
            ],
            [
                [0, 1, 2, 128],
                [0, 2, 1, 0x80],
            ]
        ]
        case_data = [[list(map(str, param_1)), list(map(str, param_2))] for param_1, param_2 in case_data]

        return case_data

    @property
    def get_unary_test_data_opposite_sign_zero(self):
        """test opposite signs of zero for unary ops"""
        case_data = [
            ['-0', '-0', '+0', '+0'],
            ['+0', '0', '-0', '0'],
            ['-0', '-0', '-0', '-0'],
            ['+0', '+0', '+0', '+0'],
        ]

        return case_data

    @property
    def get_binary_test_data_opposite_sign_zero(self):
        """test opposite signs of zero for binary ops"""
        case_data = [
            [
                ['-0', '-0', '+0', '+0'],
                ['+0', '0', '-0', '0'],
            ],
            [
                ['-0', '-0', '-0', '-0'],
                ['+0', '+0', '+0', '+0'],
            ]
        ]

        return case_data

    @property
    def get_unary_test_data(self):
        """general unary case data"""

        case_data = [

            ['1'] * self.lane_count,
            ['-1'] * self.lane_count,
            [str(self.LANE_VALUE.mask)] * self.lane_count,
            [hex(self.LANE_VALUE.mask)] * self.lane_count,
            [str(-self.LANE_VALUE.min)] * self.lane_count,
            [str(self.LANE_VALUE.min)] * self.lane_count,
            [hex(self.LANE_VALUE.min)] * self.lane_count,
            [hex(-self.LANE_VALUE.min)] * self.lane_count,
            ['01_2_3'] * self.lane_count,
            ['-01_2_3'] * self.lane_count,
            ['0x80'] * self.lane_count,
            ['-0x80'] * self.lane_count,
            ['0x0_8_0'] * self.lane_count,
            ['-0x0_8_0'] * self.lane_count

        ]

        return case_data

    @property
    def get_binary_test_data(self):
        """general binary case data"""

        case_data = [

            [
                ['0'] * self.lane_count,
                ['0'] * self.lane_count,
            ],
            [
                ['0'] * self.lane_count,
                ['-1'] * self.lane_count,
            ],
            [
                ['0', '0', '-1', '-1'],
                ['0', '-1', '0', '-1'],
            ],
            [
                ['0'] * self.lane_count,
                [hex(self.LANE_VALUE.mask)] * self.lane_count,
            ],

            [
                ['1'] * self.lane_count,
                ['1'] * self.lane_count,
            ],
            [
                [str(self.LANE_VALUE.mask)] * self.lane_count,
                ['1'] * self.lane_count,
            ],
            [
                [str(self.LANE_VALUE.mask)] * self.lane_count,
                ['128'] * self.lane_count,
            ],
            [
                [str(-self.LANE_VALUE.min)] * self.lane_count,
                [str(self.LANE_VALUE.min)] * self.lane_count,
            ],
            [
                [hex(-self.LANE_VALUE.min)] * self.lane_count,
                [str(self.LANE_VALUE.min)] * self.lane_count,
            ],
            [
                ['123'] * self.lane_count,
                ['01_2_3'] * self.lane_count,
            ],
            [
                ['0x80'] * self.lane_count,
                ['0x0_8_0'] * self.lane_count,
            ],

        ]

        return case_data

    @property
    def gen_funcs_normal(self):
        """generate normal functions"""
        binary_func_template = '\n  (func (export "{lane_type}.{op}") (param v128 v128) (result v128) ({lane_type}.{op} (local.get 0) (local.get 1)))'
        unary_func_template = '\n  (func (export "{lane_type}.{op}") (param v128) (result v128) ({lane_type}.{op} (local.get 0)))'
        funcs = ''
        for op in self.BINARY_OPS:
            funcs += binary_func_template.format(lane_type=self.LANE_TYPE, op=op)
        for op in self.UNARY_OPS:
            funcs += unary_func_template.format(lane_type=self.LANE_TYPE, op=op)
        return funcs

    @property
    def gen_funcs_with_const(self):
        """generate functions with const arguments"""
        binary_func_with_const = '\n  (func (export "{lane_type}.{op}_with_const_{cnt}") (result v128) ({lane_type}.{op} {param_1} {param_2}))'
        unary_func_with_const = '\n  (func (export "{lane_type}.{op}_with_const_{cnt}") (result v128) ({lane_type}.{op} {param}))'
        binary_func_with_param_and_const = '\n  (func (export "{lane_type}.{op}_with_const_{cnt}") (param v128) (result v128) ({lane_type}.{op} (local.get 0) {param_1}))'
        funcs = ''
        cnt = 0
        for op in self.BINARY_OPS:
            for param_1, param_2 in self.get_binary_test_data_with_const:
                funcs += binary_func_with_const.format(lane_type=self.LANE_TYPE,
                                                op=op,
                                                param_1=SIMD.v128_const(param_1, self.LANE_TYPE),
                                                param_2=SIMD.v128_const(param_2, self.LANE_TYPE),
                                                cnt=cnt)
                cnt += 1
        for op in self.UNARY_OPS:
            for param in self.get_unary_complex_test_data:
                funcs += unary_func_with_const.format(lane_type=self.LANE_TYPE,
                                               op=op,
                                               param=SIMD.v128_const(param, self.LANE_TYPE),
                                               cnt=cnt)
                cnt += 1
        for op in self.BINARY_OPS:
            for param_1, param_2 in self.get_binary_test_data_with_const:
                funcs += binary_func_with_param_and_const.format(lane_type=self.LANE_TYPE,
                                                          op=op,
                                                          param_1=SIMD.v128_const(param_1, self.LANE_TYPE),
                                                          cnt=cnt)
                cnt += 1

        return funcs

    @property
    def gen_test_case_with_const(self):
        """generate tests calling function with const"""
        cnt = 0
        cases = '\n\n;; Const vs const'
        for op in self.BINARY_OPS:
            o = ArithmeticOp(op)
            for param_1, param_2 in self.get_binary_test_data_with_const:
                result = []
                for idx in range(0, len(param_1)):
                    result.append(o.binary_op(param_1[idx], param_2[idx], self.LANE_VALUE))
                cases += '\n' + str(AssertReturn('{lane_type}.{op}_with_const_{cnt}'.format(lane_type=self.LANE_TYPE, op=op, cnt=cnt),
                                                 [],
                                                 SIMD.v128_const(result, self.LANE_TYPE)))
                cnt += 1

        for op in self.UNARY_OPS:
            o = ArithmeticOp(op)
            for param in self.get_unary_complex_test_data:
                result = []
                for idx in range(0, len(param)):
                    result.append(o.unary_op(param[idx], self.LANE_VALUE))
                cases += '\n' + str(AssertReturn('{lane_type}.{op}_with_const_{cnt}'.format(lane_type=self.LANE_TYPE, op=op, cnt=cnt),
                                                 [],
                                                 SIMD.v128_const(result, self.LANE_TYPE)))
                cnt += 1

        cases += '\n\n;; Param vs const'
        for op in self.BINARY_OPS:
            o = ArithmeticOp(op)
            for param_1, param_2 in self.get_binary_test_data_with_const:
                result = []
                for idx in range(0, len(param_1)):
                    result.append(o.binary_op(param_1[idx], param_2[idx], self.LANE_VALUE))
                cases += '\n' + str(AssertReturn('{lane_type}.{op}_with_const_{cnt}'.format(lane_type=self.LANE_TYPE, op=op, cnt=cnt),
                                                 [SIMD.v128_const(param_2, self.LANE_TYPE)],
                                                 SIMD.v128_const(result, self.LANE_TYPE)))
                cnt += 1

        return cases

    @property
    def gen_test_case(self):
        """generate binary test cases"""
        cases = ''

        def gen_binary(case_data):
            cases = ''
            for op in self.BINARY_OPS:
                o = ArithmeticOp(op)
                for param_1, param_2 in case_data:
                    result = []
                    for idx in range(0, len(param_1)):
                        result.append(o.binary_op(param_1[idx], param_2[idx], self.LANE_VALUE))
                    cases += '\n' + str(AssertReturn('{lane_type}.{op}'.format(lane_type=self.LANE_TYPE, op=op),
                                                     [SIMD.v128_const(param_1, self.LANE_TYPE), SIMD.v128_const(param_2, self.LANE_TYPE)],
                                                     SIMD.v128_const(result, self.LANE_TYPE)))
            return cases

        def gen_unary(case_data):
            cases = ''
            for op in self.UNARY_OPS:
                o = ArithmeticOp(op)
                for param in case_data:
                    result = []
                    for idx in range(0, len(param)):
                        result.append(o.unary_op(param[idx], self.LANE_VALUE))
                    cases += '\n' + str(AssertReturn('{lane_type}.{op}'.format(lane_type=self.LANE_TYPE, op=op),
                                                     [SIMD.v128_const(param, self.LANE_TYPE)],
                                                     SIMD.v128_const(result, self.LANE_TYPE)))
            return cases

        cases += gen_binary(self.get_binary_test_data)
        cases += gen_unary(self.get_unary_test_data)

        cases += self.gen_test_case_with_const

        # test different lanes go through different if-then clauses
        cases += '\n\n;; Test different lanes go through different if-then clauses'
        cases += gen_binary(self.get_binary_test_data_go_through_if)
        cases += gen_unary(self.get_unary_complex_test_data)

        # test opposite signs of zero
        cases += '\n\n;; Test opposite signs of zero'
        cases += gen_binary(self.get_binary_test_data_opposite_sign_zero)
        cases += gen_unary(self.get_unary_test_data_opposite_sign_zero)

        # unknown operators test cases
        cases += self.gen_test_case_unknown_operators

        # type check test cases
        cases += self.gen_test_case_type_check

        # empty argument test cases
        cases += self.gen_test_case_empty_argument

        return cases

    @property
    def gen_test_case_unknown_operators(self):
        """generate unknown operators test cases"""
        cases = ['\n\n;; Unknown operators']

        for op in self.UNKNOWN_BINARY_OPS:
            cases.append(AssertMalformed.get_unknown_op_test(
                op, 'v128',
                SIMD.v128_const('0', self.LANE_TYPE),
                SIMD.v128_const('1', self.LANE_TYPE)
            ))
        if hasattr(self, 'UNKNOWN_UNARY_OPS'):
            for op in self.UNKNOWN_UNARY_OPS:
                cases.append(AssertMalformed.get_unknown_op_test(
                    op, 'v128',
                    SIMD.v128_const('-1', self.LANE_TYPE)
                ))
        return '\n'.join(cases)

    @property
    def gen_test_case_type_check(self):
        """generate type check test cases"""
        cases = '\n\n;; Type check'
        binary_assert_template = '(assert_invalid (module (func (result v128) ({lane_type}.{op} (i32.const 0) (f32.const 0.0)))) "type mismatch")'
        unary_assert_template = '(assert_invalid (module (func (result v128) ({lane_type}.{op} (f32.const 0.0)))) "type mismatch")'
        for op in self.BINARY_OPS:
            cases += '\n' + binary_assert_template.format(lane_type=self.LANE_TYPE, op=op)
        for op in self.UNARY_OPS:
            cases += '\n' + unary_assert_template.format(lane_type=self.LANE_TYPE, op=op)

        return cases

    @property
    def gen_funcs_combination(self):
        """generate functions for combination test cases"""
        funcs = '\n\n;; Combination'
        funcs += '\n(module'

        binary_vs_binary_assert_template = '  (func (export "{lane_type}.{op1}-{lane_type}.{op2}") (param v128 v128 v128) (result v128) ' \
                          '({lane_type}.{op1} ({lane_type}.{op2} (local.get 0) (local.get 1))(local.get 2))' \
                          ')'
        binary_vs_unary_assert_template = '  (func (export "{lane_type}.{op1}-{lane_type}.{op2}") (param v128 v128) (result v128) ' \
                          '({lane_type}.{op1} ({lane_type}.{op2} (local.get 0))(local.get 1))' \
                          ')'
        unary_vs_binary_assert_template = '  (func (export "{lane_type}.{op1}-{lane_type}.{op2}") (param v128 v128) (result v128) ' \
                          '({lane_type}.{op1} ({lane_type}.{op2} (local.get 0) (local.get 1)))' \
                          ')'
        unary_vs_unary_assert_template = '  (func (export "{lane_type}.{op1}-{lane_type}.{op2}") (param v128) (result v128) ' \
                          '({lane_type}.{op1} ({lane_type}.{op2} (local.get 0)))' \
                          ')'

        binary_ops = list(self.BINARY_OPS)
        binary_ops.reverse()
        unary_ops = list(self.UNARY_OPS)
        unary_ops.reverse()
        for op1 in self.BINARY_OPS:
            for op2 in binary_ops:
                funcs += '\n' + binary_vs_binary_assert_template.format(lane_type=self.LANE_TYPE, op1=op1, op2=op2)
            for op2 in self.UNARY_OPS:
                funcs += '\n' + binary_vs_unary_assert_template.format(lane_type=self.LANE_TYPE, op1=op1, op2=op2)
                funcs += '\n' + unary_vs_binary_assert_template.format(lane_type=self.LANE_TYPE, op1=op2, op2=op1)
        for op1 in self.UNARY_OPS:
            for op2 in unary_ops:
                funcs += '\n' + unary_vs_unary_assert_template.format(lane_type=self.LANE_TYPE, op1=op1, op2=op2)

        funcs += '\n)'
        return funcs

    @property
    def gen_test_case_combination(self):
        """generate combination test cases"""

        cases = '\n'

        binary_ops = list(self.BINARY_OPS)
        binary_ops.reverse()
        unary_ops = list(self.UNARY_OPS)
        unary_ops.reverse()
        for op1 in self.BINARY_OPS:
            """binary vs binary"""
            o1 = ArithmeticOp(op1)
            for op2 in binary_ops:
                o2 = ArithmeticOp(op2)
                result = []
                ret = o2.binary_op('0', '1', self.LANE_VALUE)
                ret = o1.binary_op(ret, '2', self.LANE_VALUE)
                result.append(ret)

                cases += '\n' + str(AssertReturn('{lane_type}.{op1}-{lane_type}.{op2}'.format(lane_type=self.LANE_TYPE, op1=op1, op2=op2),
                                                 [SIMD.v128_const('0', self.LANE_TYPE),
                                                  SIMD.v128_const('1', self.LANE_TYPE),
                                                  SIMD.v128_const('2', self.LANE_TYPE)],
                                                 SIMD.v128_const(result, self.LANE_TYPE)))
            for op2 in self.UNARY_OPS:
                """binary vs unary"""
                o2 = ArithmeticOp(op2)
                result1 = []
                ret1 = o2.unary_op('-1', self.LANE_VALUE)
                ret1 = o1.binary_op(ret1, '0', self.LANE_VALUE)
                result1.append(ret1)
                cases += '\n' + str(AssertReturn('{lane_type}.{op1}-{lane_type}.{op2}'.format(lane_type=self.LANE_TYPE, op1=op1, op2=op2),
                                                 [SIMD.v128_const('-1', self.LANE_TYPE),
                                                  SIMD.v128_const('0', self.LANE_TYPE)],
                                                 SIMD.v128_const(result1, self.LANE_TYPE)))
                """unary vs binary"""
                result2 = []
                ret2 = o1.binary_op('0', '-1', self.LANE_VALUE)
                ret2 = o2.unary_op(ret2, self.LANE_VALUE)
                result2.append(ret2)
                cases += '\n' + str(AssertReturn('{lane_type}.{op1}-{lane_type}.{op2}'.format(lane_type=self.LANE_TYPE, op1=op2, op2=op1),
                                                 [SIMD.v128_const('0', self.LANE_TYPE),
                                                  SIMD.v128_const('-1', self.LANE_TYPE)],
                                                 SIMD.v128_const(result2, self.LANE_TYPE)))
        for op1 in self.UNARY_OPS:
            """unary vs unary"""
            o1 = ArithmeticOp(op1)
            for op2 in unary_ops:
                o2 = ArithmeticOp(op2)
                result3 = []
                ret3 = o2.unary_op('-1', self.LANE_VALUE)
                ret3 = o1.unary_op(ret3, self.LANE_VALUE)
                result3.append(ret3)
                cases += '\n' + str(AssertReturn('{lane_type}.{op1}-{lane_type}.{op2}'.format(lane_type=self.LANE_TYPE, op1=op1, op2=op2),
                                                 [SIMD.v128_const('-1', self.LANE_TYPE)],
                                                 SIMD.v128_const(result3, self.LANE_TYPE)))

        cases += '\n'
        return cases

    @property
    def gen_test_case_empty_argument(self):
        """generate empty argument test cases"""

        cases = []

        cases.append('\n\n;; Test operation with empty argument\n')

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

        for op in self.UNARY_OPS:
            case_data['op'] = '{lane_type}.{op}'.format(lane_type=self.LANE_TYPE, op=op)

            case_data['extended_name'] = 'arg-empty'
            case_data['params'] = ''
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

        return '\n'.join(cases)

    @property
    def gen_funcs(self):
        """generate functions"""
        funcs = ''
        funcs += '\n\n(module'
        funcs += self.gen_funcs_normal
        funcs += self.gen_funcs_with_const
        funcs += '\n)\n'

        return funcs

    def get_all_cases(self):
        """generate all test cases"""
        cases = self.class_summary.format(lane_type=self.LANE_TYPE) \
            + self.gen_funcs \
            + self.gen_test_case \
            + self.gen_funcs_combination \
            + self.gen_test_case_combination

        return cases

    def gen_test_cases(self):
        """generate case file"""
        wast_filename = '../simd_{lane_type}_arith2.wast'.format(lane_type=self.LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())


class Simdi64x2Case(SimdLaneWiseInteger):
    LANE_TYPE = 'i64x2'
    class_summary = """;; Tests for {lane_type} [abs] operations."""
    BINARY_OPS = ()

    UNKNOWN_BINARY_OPS = ()


class Simdi32x4Case(SimdLaneWiseInteger):
    LANE_TYPE = 'i32x4'
    class_summary = """;; Tests for {lane_type} [min_s, min_u, max_s, max_u, abs] operations."""

    UNKNOWN_BINARY_OPS = ('f32x4.min_s', 'f32x4.min_u', 'f32x4.max_s', 'f32x4.max_u',
                   'i64x2.min_s', 'i64x2.min_u', 'i64x2.max_s', 'i64x2.max_u',
                   'f64x2.min_s', 'f64x2.min_u', 'f64x2.max_s', 'f64x2.max_u')


class Simdi16x8Case(SimdLaneWiseInteger):
    LANE_TYPE = 'i16x8'

    BINARY_OPS = ('min_s', 'min_u', 'max_s', 'max_u', 'avgr_u')
    UNKNOWN_BINARY_OPS = ('i16x8.avgr', 'i16x8.avgr_s')


class Simdi8x16Case(SimdLaneWiseInteger):
    LANE_TYPE = 'i8x16'

    UNARY_OPS = ('abs','popcnt')
    BINARY_OPS = ('min_s', 'min_u', 'max_s', 'max_u', 'avgr_u')
    UNKNOWN_BINARY_OPS = ('i32x4.avgr_u', 'f32x4.avgr_u',
                   'i64x2.avgr_u', 'f64x2.avgr_u',
                   'i8x16.avgr', 'i8x16.avgr_s')


def gen_test_cases():
    simd_i64x2_case = Simdi64x2Case()
    simd_i64x2_case.gen_test_cases()

    simd_i32x4_case = Simdi32x4Case()
    simd_i32x4_case.gen_test_cases()

    simd_i16x8_case = Simdi16x8Case()
    simd_i16x8_case.gen_test_cases()

    simd_i8x16_case = Simdi8x16Case()
    simd_i8x16_case.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
