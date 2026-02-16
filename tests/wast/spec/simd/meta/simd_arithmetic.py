#!/usr/bin/env python3

"""Base class for generating cases integer and floating-point numbers
arithmetic and saturate arithmetic operations.

Class SimdArithmeticCase is the base class of all kinds of arithmetic
operation cases. It provides a skeleton to generate the normal, invalid and
combined cases. Subclasses only provide the test data sets. In some special
cases, you may need to override the methods in base class to fulfill your
case generation.
"""

from simd import SIMD
from test_assert import AssertReturn, AssertInvalid
from simd_lane_value import LaneValue
from simd_integer_op import ArithmeticOp


i8 = LaneValue(8)
i16 = LaneValue(16)
i32 = LaneValue(32)
i64 = LaneValue(64)


class SimdArithmeticCase:

    UNARY_OPS = ('neg',)
    BINARY_OPS = ('add', 'sub', 'mul')
    LANE_VALUE = {'i8x16': i8, 'i16x8': i16, 'i32x4': i32, 'i64x2': i64}

    TEST_FUNC_TEMPLATE_HEADER = (
            ';; Tests for {} arithmetic operations on major boundary values and all special values.\n\n')

    def op_name(self, op):
        """ Full instruction name.
        Subclasses can overwrite to provide custom instruction names that don't
        fit the default of {shape}.{op}.
        """
        return '{lane_type}.{op}'.format(lane_type=self.LANE_TYPE, op=op)

    def __str__(self):
        return self.get_all_cases()

    @property
    def lane(self):
        return self.LANE_VALUE.get(self.LANE_TYPE)

    @property
    def dst_lane(self):
        return self.lane

    @property
    def src_lane(self):
        # Used for arithmetic that extends the lane, e.g. i16x8 lanes, which
        # are extended multiply to i32x4.
        if hasattr(self, 'SRC_LANE_TYPE'):
            return self.LANE_VALUE.get(self.SRC_LANE_TYPE)
        else:
            return self.lane

    @property
    def normal_unary_op_test_data(self):
        lane = self.src_lane
        return [0, 1, -1, lane.max - 1, lane.min + 1, lane.min, lane.max, lane.mask]

    @property
    def normal_binary_op_test_data(self):
        lane = self.src_lane
        return [
            (0, 0),
            (0, 1),
            (1, 1),
            (0, -1),
            (1, -1),
            (-1, -1),
            (lane.quarter - 1, lane.quarter),
            (lane.quarter, lane.quarter),
            (-lane.quarter + 1, -lane.quarter),
            (-lane.quarter, -lane.quarter),
            (-lane.quarter - 1, -lane.quarter),
            (lane.max - 2, 1),
            (lane.max - 1, 1),
            (-lane.min, 1),
            (lane.min + 2, -1),
            (lane.min + 1, -1),
            (lane.min, -1),
            (lane.max, lane.max),
            (lane.min, lane.min),
            (lane.min, lane.min + 1),
            (lane.mask, 0),
            (lane.mask, 1),
            (lane.mask, -1),
            (lane.mask, lane.max),
            (lane.mask, lane.min),
            (lane.mask, lane.mask)
        ]

    @property
    def bin_test_data(self):
        return [
            (self.normal_binary_op_test_data, [self.LANE_TYPE] * 3),
            (self.hex_binary_op_test_data, [self.LANE_TYPE] * 3)
        ]

    @property
    def unary_test_data(self):
        return [
            (self.normal_unary_op_test_data, [self.LANE_TYPE] * 2),
            (self.hex_unary_op_test_data, [self.LANE_TYPE] * 2)
        ]

    @property
    def combine_ternary_arith_test_data(self):
        return {
            'add-sub': [
                [str(i) for i in range(self.LANE_LEN)],
                [str(i * 2) for i in range(self.LANE_LEN)],
                [str(i * 2) for i in range(self.LANE_LEN)],
                [str(i) for i in range(self.LANE_LEN)]
            ],
            'sub-add': [
                [str(i) for i in range(self.LANE_LEN)],
                [str(i * 2) for i in range(self.LANE_LEN)],
                [str(i * 2) for i in range(self.LANE_LEN)],
                [str(i) for i in range(self.LANE_LEN)]
            ],
            'mul-add': [
                [str(i) for i in range(self.LANE_LEN)],
                [str(i) for i in range(self.LANE_LEN)],
                ['2'] * self.LANE_LEN,
                [str(i * 4) for i in range(self.LANE_LEN)]
            ],
            'mul-sub': [
                [str(i * 2) for i in range(self.LANE_LEN)],
                [str(i) for i in range(self.LANE_LEN)],
                [str(i) for i in range(self.LANE_LEN)],
                [str(pow(i, 2)) for i in range(self.LANE_LEN)]
            ]
        }

    @property
    def combine_binary_arith_test_data(self):
        return {
            'add-neg': [
                [str(i) for i in range(self.LANE_LEN)],
                [str(i) for i in range(self.LANE_LEN)],
                ['0'] * self.LANE_LEN
            ],
            'sub-neg': [
                [str(i) for i in range(self.LANE_LEN)],
                [str(i) for i in range(self.LANE_LEN)],
                [str(-i * 2) for i in range(self.LANE_LEN)]
            ],
            'mul-neg': [
                [str(i) for i in range(self.LANE_LEN)],
                ['2'] * self.LANE_LEN,
                [str(-i * 2) for i in range(self.LANE_LEN)]
            ]
        }

    def gen_test_func_template(self):
        template = [
                self.TEST_FUNC_TEMPLATE_HEADER.format(self.LANE_TYPE), '(module']

        for op in self.BINARY_OPS:
            template.append('  (func (export "{op}") (param v128 v128) (result v128) '
                            '({op} (local.get 0) (local.get 1)))'.format(op=self.op_name(op)))
        for op in self.UNARY_OPS:
            template.append('  (func (export "{op}") (param v128) (result v128) '
                            '({op} (local.get 0)))'.format(op=self.op_name(op)))

        template.append(')\n')
        return template

    def gen_test_template(self):
        template = self.gen_test_func_template()

        template.append('{normal_cases}')
        template.append('\n{invalid_cases}')
        template.append('\n{combine_cases}')

        return '\n'.join(template)

    def get_case_data(self):
        case_data = []

        # i8x16.op (i8x16) (i8x16)
        for op in self.BINARY_OPS:
            o = ArithmeticOp(op)
            op_name = self.LANE_TYPE + '.' + op
            case_data.append(['#', op_name])
            for data_group, v128_forms in self.bin_test_data:
                for data in data_group:
                    case_data.append([op_name, [str(data[0]), str(data[1])],
                                      str(o.binary_op(data[0], data[1], self.src_lane, self.dst_lane)),
                                     v128_forms])
            for data_group in self.full_bin_test_data:
                for data in data_group.get(op_name):
                    case_data.append([op_name, *data])

        for op in self.UNARY_OPS:
            o = ArithmeticOp(op)
            op_name = self.LANE_TYPE + '.' + op
            case_data.append(['#', op_name])
            for data_group, v128_forms in self.unary_test_data:
                for data in data_group:
                    case_data.append([op_name, [str(data)],
                                      str(o.unary_op(data, self.dst_lane)),
                                      v128_forms])

        return case_data

    def get_invalid_cases(self):
        invalid_cases = [';; type check']

        unary_template = '(assert_invalid (module (func (result v128) '\
                         '({name} ({operand})))) "type mismatch")'
        binary_template = '(assert_invalid (module (func (result v128) '\
                          '({name} ({operand_1}) ({operand_2})))) "type mismatch")'


        for op in self.UNARY_OPS:
            invalid_cases.append(unary_template.format(name=self.op_name(op),
                                                       operand='i32.const 0'))
        for op in self.BINARY_OPS:
            invalid_cases.append(binary_template.format(name=self.op_name(op),
                                                        operand_1='i32.const 0',
                                                        operand_2='f32.const 0.0'))

        return '\n'.join(invalid_cases) + self.argument_empty_test()

    def argument_empty_test(self):
        """Test cases with empty argument.
        """
        cases = []

        cases.append('\n\n;; Test operation with empty argument\n')

        case_data = {
            'op': '',
            'extended_name': 'arg-empty',
            'param_type': '',
            'result_type': '(result v128)',
            'params': '',
        }

        for op in self.UNARY_OPS:
            case_data['op'] = self.op_name(op)
            case_data['extended_name'] = 'arg-empty'
            case_data['params'] = ''
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

        for op in self.BINARY_OPS:
            case_data['op'] = self.op_name(op)
            case_data['extended_name'] = '1st-arg-empty'
            case_data['params'] = SIMD.v128_const('0', self.LANE_TYPE)
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

            case_data['extended_name'] = 'arg-empty'
            case_data['params'] = ''
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

        return '\n'.join(cases)

    def get_combine_cases(self):
        combine_cases = [';; combination\n(module']
        ternary_func_template = '  (func (export "{func}") (param v128 v128 v128) (result v128)\n' \
                              '    ({lane}.{op1} ({lane}.{op2} (local.get 0) (local.get 1))'\
                              '(local.get 2)))'
        for func in sorted(self.combine_ternary_arith_test_data):
            func_parts = func.split('-')
            combine_cases.append(ternary_func_template.format(func=func,
                                                            lane=self.LANE_TYPE,
                                                            op1=func_parts[0],
                                                            op2=func_parts[1]))
        binary_func_template = '  (func (export "{func}") (param v128 v128) (result v128)\n'\
                             '    ({lane}.{op1} ({lane}.{op2} (local.get 0)) (local.get 1)))'
        for func in sorted(self.combine_binary_arith_test_data):
            func_parts = func.split('-')
            combine_cases.append(binary_func_template.format(func=func,
                                                           lane=self.LANE_TYPE,
                                                           op1=func_parts[0],
                                                           op2=func_parts[1]))
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

    def get_normal_case(self):
        s = SIMD()
        case_data = self.get_case_data()
        cases = []

        for item in case_data:
            # Recognize '#' as a commentary
            if item[0] == '#':
                cases.append('\n;; {}'.format(item[1]))
                continue

            instruction, param, ret, lane_type = item
            v128_result = s.v128_const(ret, lane_type[-1])
            v128_params = []
            for i, p in enumerate(param):
                v128_params.append(s.v128_const(p, lane_type[i]))
            cases.append(str(AssertReturn(instruction, v128_params, v128_result)))

        return '\n'.join(cases)

    def get_all_cases(self):
        case_data = {'lane_type': self.LANE_TYPE,
                     'normal_cases': self.get_normal_case(),
                     'invalid_cases': self.get_invalid_cases(),
                     'combine_cases': self.get_combine_cases()
                     }
        return self.gen_test_template().format(**case_data)

    def gen_test_cases(self):
        wast_filename = '../simd_{lane_type}_arith.wast'.format(lane_type=self.LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())
