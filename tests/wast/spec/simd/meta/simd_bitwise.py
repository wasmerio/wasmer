#!/usr/bin/env python3

"""
This file is used for generating bitwise test cases
"""

from simd import SIMD
from test_assert import AssertReturn, AssertInvalid


class SimdBitWise(SIMD):
    """
    Generate common tests
    """

    UNARY_OPS = ('not',)
    BINARY_OPS = ('and', 'or', 'xor', 'andnot',)
    TERNARY_OPS = ('bitselect',)

    # Test case template
    CASE_TXT = """;; Test all the bitwise operators on major boundary values and all special values.

(module
  (func (export "not") (param $0 v128) (result v128) (v128.not (local.get $0)))
  (func (export "and") (param $0 v128) (param $1 v128) (result v128) (v128.and (local.get $0) (local.get $1)))
  (func (export "or") (param $0 v128) (param $1 v128) (result v128) (v128.or (local.get $0) (local.get $1)))
  (func (export "xor") (param $0 v128) (param $1 v128) (result v128) (v128.xor (local.get $0) (local.get $1)))
  (func (export "bitselect") (param $0 v128) (param $1 v128) (param $2 v128) (result v128)
    (v128.bitselect (local.get $0) (local.get $1) (local.get $2))
  )
  (func (export "andnot") (param $0 v128) (param $1 v128) (result v128) (v128.andnot (local.get $0) (local.get $1)))
)
{normal_case}"""

    @staticmethod
    def init_case_data(case_data):
        """
        Rearrange const data into standard format
        e.g. [0][i32x4] => (v128.const i32x4 0 0 0 0)
             [0][i32]   => (i32.const 0)
        """

        s_i = SIMD()

        lst_i_p_r = []

        for item in case_data:
            # Recognize '#' as a commentary
            if item[0] == '#':
                comment = '\n' if len(item[1]) == 0 else '\n;; {}'.format(item[1])
                lst_i_p_r.append(['#', comment])
                continue

            # Params: instruction: instruction name;
            #         params: param for instruction;
            #         rets: excepted result;
            #         lane_type: lane type for param and ret
            instruction, params, rets, lane_type = item

            p_const_list = []
            for idx, param in enumerate(params):
                p_const_list.append(s_i.v128_const(param, lane_type[idx]))

            r_const_list = []
            for idx, ret in enumerate(rets):
                r_const_list.append(s_i.v128_const(ret, lane_type[idx + len(params)]))

            lst_i_p_r.append([instruction, p_const_list, r_const_list])

        return lst_i_p_r

    # Generate normal case with test datas
    def get_normal_case(self):
        """
        Generate normal case with test data
        """

        lst_i_p_r = self.init_case_data(self.get_case_data())

        cases = []
        for ipr in lst_i_p_r:

            if ipr[0] == '#':
                cases.append(ipr[1])
                continue

            cases.append(str(AssertReturn(ipr[0],
                                          ipr[1],
                                          ipr[2])))

        return '\n'.join(cases)

    def get_invalid_case(self):
        """
        Generate invalid case with test data
        """

        case_data = [
            # i8x16
            ['#', 'Type check'],
            ['#', ''],

            ['#', 'not'],
            ["v128.not", ['0'], [], ['i32']],

            ['#', 'and'],
            ["v128.and", ['0', '0'], [], ['i32', 'i32x4']],
            ["v128.and", ['0', '0'], [], ['i32x4', 'i32']],
            ["v128.and", ['0', '0'], [], ['i32', 'i32']],

            ['#', 'or'],
            ["v128.or", ['0', '0'], [], ['i32', 'i32x4']],
            ["v128.or", ['0', '0'], [], ['i32x4', 'i32']],
            ["v128.or", ['0', '0'], [], ['i32', 'i32']],

            ['#', 'xor'],
            ["v128.xor", ['0', '0'], [], ['i32', 'i32x4']],
            ["v128.xor", ['0', '0'], [], ['i32x4', 'i32']],
            ["v128.xor", ['0', '0'], [], ['i32', 'i32']],

            ['#', 'bitselect'],
            ["v128.bitselect", ['0', '0', '0'], [], ['i32', 'i32x4', 'i32x4']],
            ["v128.bitselect", ['0', '0', '0'], [], ['i32x4', 'i32x4', 'i32']],
            ["v128.bitselect", ['0', '0', '0'], [], ['i32', 'i32', 'i32']],

            ['#', 'andnot'],
            ["v128.andnot", ['0', '0'], [], ['i32', 'i32x4']],
            ["v128.andnot", ['0', '0'], [], ['i32x4', 'i32']],
            ["v128.andnot", ['0', '0'], [], ['i32', 'i32']]
        ]

        lst_ipr = self.init_case_data(case_data)

        str_invalid_case_func_tpl = '\n(assert_invalid (module (func (result v128)' \
                                    ' ({op} {operand}))) "type mismatch")'

        lst_invalid_case_func = []

        for ipr in lst_ipr:

            if ipr[0] == '#':
                lst_invalid_case_func.append(ipr[1])
                continue
            else:
                lst_invalid_case_func.append(
                    str_invalid_case_func_tpl.format(op=ipr[0], operand=' '.join(ipr[1]))
                )

        return '\n{}\n'.format(''.join(lst_invalid_case_func))

    def get_combination_case(self):
        """
        Generate combination case with test data
        """

        str_in_block_case_func_tpl = '\n  (func (export "{op}-in-block")' \
                                     '\n    (block' \
                                     '\n      (drop' \
                                     '\n        (block (result v128)' \
                                     '\n          ({op}' \
                                     '{block_with_result}' \
                                     '\n          )' \
                                     '\n        )' \
                                     '\n      )' \
                                     '\n    )' \
                                     '\n  )'
        str_nested_case_func_tpl = '\n  (func (export "nested-{op}")' \
                                   '\n    (drop' \
                                   '\n      ({op}' \
                                   '{block_with_result}' \
                                   '\n      )' \
                                   '\n    )' \
                                   '\n  )'

        case_data = [
            ["v128.not", ['0'], [], ['i32']],
            ["v128.and", ['0', '1'], [], ['i32', 'i32']],
            ["v128.or", ['0', '1'], [], ['i32', 'i32']],
            ["v128.xor", ['0', '1'], [], ['i32', 'i32']],
            ["v128.bitselect", ['0', '1', '2'], [], ['i32', 'i32', 'i32']],
            ["v128.andnot", ['0', '1'], [], ['i32', 'i32']],
        ]
        lst_ipr = self.init_case_data(case_data)

        lst_in_block_case_func = []
        lst_nested_case_func = []
        lst_in_block_case_assert = []
        lst_nested_case_assert = []
        lst_argument_empty_case = []

        for ipr in lst_ipr:

            lst_block = ['\n            (block (result v128) (v128.load {}))'.format(x) for x in ipr[1]]
            lst_in_block_case_func.append(
                str_in_block_case_func_tpl.format(op=ipr[0], block_with_result=''.join(lst_block))
            )

            tpl_1 = '\n        ({op}' \
                    '{combined_operation}' \
                    '\n        )'
            tpl_2 = '\n          ({op}' \
                    '{combined_operation}' \
                    '\n          )'
            tpl_3 = '\n            (v128.load {value})'

            lst_tpl_3 = [tpl_3.format(value=x) for x in ipr[1]]
            lst_tpl_2 = [tpl_2.format(op=ipr[0], combined_operation=''.join(lst_tpl_3))] * len(ipr[1])
            lst_tpl_1 = [tpl_1.format(op=ipr[0], combined_operation=''.join(lst_tpl_2))] * len(ipr[1])

            lst_nested_case_func.append(
                str_nested_case_func_tpl.format(op=ipr[0], block_with_result=''.join(lst_tpl_1))
            )

            lst_in_block_case_assert.append('\n(assert_return (invoke "{}-in-block"))'.format(ipr[0]))
            lst_nested_case_assert.append('\n(assert_return (invoke "nested-{}"))'.format(ipr[0]))

        return '\n;; Combination\n' \
               '\n(module (memory 1)' \
               '{in_block_cases}' \
               '{nested_cases}' \
               '\n  (func (export "as-param")' \
               '\n    (drop' \
               '\n      (v128.or' \
               '\n        (v128.and' \
               '\n          (v128.not' \
               '\n            (v128.load (i32.const 0))' \
               '\n          )' \
               '\n          (v128.not' \
               '\n            (v128.load (i32.const 1))' \
               '\n          )' \
               '\n        )' \
               '\n        (v128.xor' \
               '\n          (v128.bitselect' \
               '\n            (v128.load (i32.const 0))' \
               '\n            (v128.load (i32.const 1))' \
               '\n            (v128.load (i32.const 2))' \
               '\n          )' \
               '\n          (v128.andnot' \
               '\n            (v128.load (i32.const 0))' \
               '\n            (v128.load (i32.const 1))' \
               '\n          )' \
               '\n        )' \
               '\n      )' \
               '\n    )' \
               '\n  )' \
               '\n)' \
               '{assert_in_block_cases}' \
               '{assert_of_nested_cases}' \
               '\n(assert_return (invoke "as-param"))\n'.format(in_block_cases=''.join(lst_in_block_case_func),
                                                    nested_cases=''.join(lst_nested_case_func),
                                                    assert_in_block_cases=''.join(lst_in_block_case_assert),
                                                    assert_of_nested_cases=''.join(lst_nested_case_assert))

    def get_argument_empty_case(self):
        """
        Generate argument empty cases
        """

        cases = []

        param_1 = SIMD.v128_const('0', 'i32x4')

        cases.append('\n\n;; Test operation with empty argument\n')

        case_data = {
            'op': '',
            'extended_name': 'arg-empty',
            'param_type': '',
            'result_type': '(result v128)',
            'params': '',
        }

        for op in self.UNARY_OPS:
            case_data['op'] = 'v128.' + op
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

        for op in self.BINARY_OPS:
            case_data['op'] = 'v128.' + op
            case_data['extended_name'] = '1st-arg-empty'
            case_data['params'] = param_1
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

            case_data['extended_name'] = 'arg-empty'
            case_data['params'] = ''
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

        for op in self.TERNARY_OPS:
            case_data['op'] = 'v128.' + op
            case_data['extended_name'] = '1st-arg-empty'
            case_data['params'] = param_1 + ' ' + param_1
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

            case_data['extended_name'] = 'two-args-empty'
            case_data['params'] = param_1
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

            case_data['extended_name'] = 'arg-empty'
            case_data['params'] = ''
            cases.append(AssertInvalid.get_arg_empty_test(**case_data))

        return '\n'.join(cases) + '\n'


    def get_all_cases(self):
        """
        generate all test cases
        """

        case_data = {'normal_case': self.get_normal_case()}

        # Add tests for unkonow operators for i32x4
        return self.CASE_TXT.format(**case_data) + self.get_invalid_case() + self.get_combination_case() + self.get_argument_empty_case()

    def get_case_data(self):
        """
        Overload base class method and set test data for bitwise.
        """
        return [
            # i32x4
            ['#', 'i32x4'],
            ["not", ['0'], ['-1'], ['i32x4', 'i32x4']],
            ["not", ['-1'], ['0'], ['i32x4', 'i32x4']],
            ["not", [['-1', '0', '-1', '0']], [['0', '-1', '0', '-1']], ['i32x4', 'i32x4']],
            ["not", [['0', '-1', '0', '-1']], [['-1', '0', '-1', '0']], ['i32x4', 'i32x4']],
            ["not", ['0x55555555'], ['0xAAAAAAAA'], ['i32x4', 'i32x4']],
            ["not", ['3435973836'], ['858993459'], ['i32x4', 'i32x4']],
            ['not', ['01_234_567_890'], ['3060399405'], ['i32x4', 'i32x4']],
            ['not', ['0x0_1234_5678'], ['0xedcba987'], ['i32x4', 'i32x4']],
            ["and", [['0', '-1'], ['0', '-1', '0', '-1']], [['0', '0', '0', '-1']], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['0', '0'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['0', '-1'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['0', '0xFFFFFFFF'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['1', '1'], ['1'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['255', '85'], ['85'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['255', '128'], ['128'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['2863311530', ['10', '128', '5', '165']], [['10', '128', '0', '160']],
                    ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['0xFFFFFFFF', '0x55555555'], ['0x55555555'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['0xFFFFFFFF', '0xAAAAAAAA'], ['0xAAAAAAAA'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['0xFFFFFFFF', '0x0'], ['0x0'], ['i32x4', 'i32x4', 'i32x4']],
            ["and", ['0x55555555', ['0x5555', '0xFFFF', '0x55FF', '0x5FFF']], ['0x5555'],
                    ['i32x4', 'i32x4', 'i32x4']],
            ['and', ['01_234_567_890', '01_234_567_890'], ['1234567890'], ['i32x4', 'i32x4', 'i32x4']],
            ['and', ['0x0_1234_5678', '0x0_90AB_cdef'], ['0x10204468'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", [['0', '0', '-1', '-1'], ['0', '-1', '0', '-1']], [['0', '-1', '-1', '-1']],
                   ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['0', '0'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['0', '-1'], ['-1'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['0', '0xFFFFFFFF'], ['0xFFFFFFFF'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['1', '1'], ['1'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['255', '85'], ['255'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['255', '128'], ['255'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['2863311530', ['10', '128', '5', '165']], [['2863311530', '2863311535']],
                   ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['0xFFFFFFFF', '0x55555555'], ['0xFFFFFFFF'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['0xFFFFFFFF', '0xAAAAAAAA'], ['0xFFFFFFFF'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['0xFFFFFFFF', '0x0'], ['0xFFFFFFFF'], ['i32x4', 'i32x4', 'i32x4']],
            ["or", ['0x55555555', ['0x5555', '0xFFFF', '0x55FF', '0x5FFF']],
                   [['0x55555555', '0x5555ffff', '0x555555ff', '0x55555fff']],
                   ['i32x4', 'i32x4', 'i32x4']],
            ['or', ['01_234_567_890', '01_234_567_890'], ['1234567890'], ['i32x4', 'i32x4', 'i32x4']],
            ['or', ['0x0_1234_5678', '0x0_90AB_cdef'], ['0x92bfdfff'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", [['0', '0', '-1', '-1'], ['0', '-1', '0', '-1']], [['0', '-1', '-1', '0']],
                    ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['0', '0'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['0', '-1'], ['-1'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['0', '0xFFFFFFFF'], ['0xFFFFFFFF'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['1', '1'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['255', '85'], ['170'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['255', '128'], ['127'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['2863311530', ['10', '128', '5', '165']],
                    [['2863311520', '2863311402', '2863311535', '2863311375']],
                    ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['0xFFFFFFFF', '0x55555555'], ['0xAAAAAAAA'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['0xFFFFFFFF', '0xAAAAAAAA'], ['0x55555555'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['0xFFFFFFFF', '0x0'], ['0xFFFFFFFF'], ['i32x4', 'i32x4', 'i32x4']],
            ["xor", ['0x55555555', ['0x5555', '0xFFFF', '0x55FF', '0x5FFF']],
                    [['0x55550000', '0x5555AAAA', '0x555500AA', '0x55550AAA']],
                    ['i32x4', 'i32x4', 'i32x4']],
            ['xor', ['01_234_567_890', '01_234_567_890'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ['xor', ['0x0_1234_5678', '0x0_90AB_cdef'], ['0x829f9b97'], ['i32x4', 'i32x4', 'i32x4']],
            ["bitselect", ['0xAAAAAAAA', '0xBBBBBBBB',
                           ['0x00112345', '0xF00FFFFF', '0x10112021', '0xBBAABBAA']],
                          [['0xBBAABABA', '0xABBAAAAA', '0xABAABBBA', '0xAABBAABB']],
                          ['i32x4', 'i32x4', 'i32x4', 'i32x4']],
            ["bitselect", ['0xAAAAAAAA', '0xBBBBBBBB', '0x00000000'], ['0xBBBBBBBB'],
                          ['i32x4', 'i32x4', 'i32x4', 'i32x4']],
            ["bitselect", ['0xAAAAAAAA', '0xBBBBBBBB', '0x11111111'], ['0xAAAAAAAA'],
                          ['i32x4', 'i32x4', 'i32x4', 'i32x4']],
            ["bitselect", ['0xAAAAAAAA', '0xBBBBBBBB',
                           ['0x01234567', '0x89ABCDEF', '0xFEDCBA98', '0x76543210']],
                          [['0xBABABABA', '0xABABABAB']],
                          ['i32x4', 'i32x4', 'i32x4', 'i32x4']],
            ["bitselect", ['0xAAAAAAAA', '0x55555555',
                           ['0x01234567', '0x89ABCDEF', '0xFEDCBA98', '0x76543210']],
                          [['0x54761032', '0xDCFE98BA', '0xAB89EFCD', '0x23016745']],
                          ['i32x4', 'i32x4', 'i32x4', 'i32x4']],
            ["bitselect", ['0xAAAAAAAA', '0x55555555',
                           ['0x55555555', '0xAAAAAAAA', '0x00000000', '0xFFFFFFFF']],
                          [['0x00000000', '0xFFFFFFFF', '0x55555555', '0xAAAAAAAA']],
                          ['i32x4', 'i32x4', 'i32x4', 'i32x4']],
            ['bitselect', ['01_234_567_890', '03_060_399_406', '0xcdefcdef'], ['2072391874'],
                          ['i32x4', 'i32x4', 'i32x4', 'i32x4']],
            ['bitselect', ['0x0_1234_5678', '0x0_90AB_cdef', '0xcdefcdef'], ['0x10244468'],
                          ['i32x4', 'i32x4', 'i32x4', 'i32x4']],
            ["andnot", [['0', '-1'], ['0', '-1', '0', '-1']], [['0', '0', '-1', '0']], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['0', '0'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['0', '-1'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['0', '0xFFFFFFFF'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['1', '1'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['255', '85'], ['170'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['255', '128'], ['127'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['2863311530', ['10', '128', '5', '165']], [['2863311520', '2863311402', '2863311530', '2863311370']],
                       ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['0xFFFFFFFF', '0x55555555'], ['0xAAAAAAAA'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['0xFFFFFFFF', '0xAAAAAAAA'], ['0x55555555'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['0xFFFFFFFF', '0x0'], ['0xFFFFFFFF'], ['i32x4', 'i32x4', 'i32x4']],
            ["andnot", ['0x55555555', ['0x5555', '0xFFFF', '0x55FF', '0x5FFF']], ['0x55550000'],
                       ['i32x4', 'i32x4', 'i32x4']],
            ['andnot', ['01_234_567_890', '01_234_567_890'], ['0'], ['i32x4', 'i32x4', 'i32x4']],
            ['andnot', ['0x0_1234_5678', '0x0_90AB_cdef'], ['0x02141210'], ['i32x4', 'i32x4', 'i32x4']],

            ['#', 'for float special data [e.g. -nan nan -inf inf]'],
            ["not", ['-nan'], ['5.87747e-39'], ['f32x4', 'f32x4']],
            ["not", ['nan'], ['-5.87747e-39'], ['f32x4', 'f32x4']],
            ["not", ['-inf'], ['0x007fffff'], ['f32x4', 'i32x4']],
            ["not", ['inf'], ['0x807fffff'], ['f32x4', 'i32x4']],

            ["and", ['-nan', '-nan'], ['0xffc00000'], ['f32x4', 'f32x4', 'i32x4']],
            ["and", ['-nan', 'nan'], ['nan'], ['f32x4', 'f32x4', 'f32x4']],
            ["and", ['-nan', '-inf'], ['-inf'], ['f32x4', 'f32x4', 'f32x4']],
            ["and", ['-nan', 'inf'], ['inf'], ['f32x4', 'f32x4', 'f32x4']],
            ["and", ['nan', 'nan'], ['nan'], ['f32x4', 'f32x4', 'f32x4']],
            ["and", ['nan', '-inf'], ['inf'], ['f32x4', 'f32x4', 'f32x4']],
            ["and", ['nan', 'inf'], ['inf'], ['f32x4', 'f32x4', 'f32x4']],
            ["and", ['-inf', '-inf'], ['-inf'], ['f32x4', 'f32x4', 'f32x4']],
            ["and", ['-inf', 'inf'], ['inf'], ['f32x4', 'f32x4', 'f32x4']],
            ["and", ['inf', 'inf'], ['inf'], ['f32x4', 'f32x4', 'f32x4']],

            ["or", ['-nan', '-nan'], ['0xffc00000'], ['f32x4', 'f32x4', 'i32x4']],
            ["or", ['-nan', 'nan'], ['0xffc00000'], ['f32x4', 'f32x4', 'i32x4']],
            ["or", ['-nan', '-inf'], ['0xffc00000'], ['f32x4', 'f32x4', 'i32x4']],
            ["or", ['-nan', 'inf'], ['0xffc00000'], ['f32x4', 'f32x4', 'i32x4']],
            ["or", ['nan', 'nan'], ['nan'], ['f32x4', 'f32x4', 'f32x4']],
            ["or", ['nan', '-inf'], ['0xffc00000'], ['f32x4', 'f32x4', 'i32x4']],
            ["or", ['nan', 'inf'], ['nan'], ['f32x4', 'f32x4', 'f32x4']],
            ["or", ['-inf', '-inf'], ['-inf'], ['f32x4', 'f32x4', 'f32x4']],
            ["or", ['-inf', 'inf'], ['-inf'], ['f32x4', 'f32x4', 'f32x4']],
            ["or", ['inf', 'inf'], ['inf'], ['f32x4', 'f32x4', 'f32x4']],

            ["xor", ['-nan', '-nan'], ['0'], ['f32x4', 'f32x4', 'f32x4']],
            ["xor", ['-nan', 'nan'], ['-0'], ['f32x4', 'f32x4', 'f32x4']],
            ["xor", ['-nan', '-inf'], ['0x00400000'], ['f32x4', 'f32x4', 'i32x4']],
            ["xor", ['-nan', 'inf'], ['0x80400000'], ['f32x4', 'f32x4', 'i32x4']],
            ["xor", ['nan', 'nan'], ['0'], ['f32x4', 'f32x4', 'f32x4']],
            ["xor", ['nan', '-inf'], ['0x80400000'], ['f32x4', 'f32x4', 'i32x4']],
            ["xor", ['nan', 'inf'], ['0x00400000'], ['f32x4', 'f32x4', 'i32x4']],
            ["xor", ['-inf', '-inf'], ['0'], ['f32x4', 'f32x4', 'f32x4']],
            ["xor", ['-inf', 'inf'], ['0x80000000'], ['f32x4', 'f32x4', 'i32x4']],
            ["xor", ['inf', 'inf'], ['0'], ['f32x4', 'f32x4', 'f32x4']],

            ["bitselect", ['-nan', '-nan','0xA5A5A5A5'], ['0xffc00000'], ['f32x4', 'f32x4', 'f32x4', 'i32x4']],
            ["bitselect", ['-nan', 'nan','0xA5A5A5A5'], ['nan'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],
            ["bitselect", ['-nan', '-inf','0xA5A5A5A5'], ['-inf'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],
            ["bitselect", ['-nan', 'inf','0xA5A5A5A5'], ['inf'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],
            ["bitselect", ['nan', 'nan','0xA5A5A5A5'], ['nan'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],
            ["bitselect", ['nan', '-inf','0xA5A5A5A5'], ['-inf'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],
            ["bitselect", ['nan', 'inf','0xA5A5A5A5'], ['inf'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],
            ["bitselect", ['-inf', '-inf','0xA5A5A5A5'], ['-inf'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],
            ["bitselect", ['-inf', 'inf','0xA5A5A5A5'], ['inf'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],
            ["bitselect", ['inf', 'inf','0xA5A5A5A5'], ['inf'], ['f32x4', 'f32x4', 'f32x4', 'f32x4']],

            ["andnot", ['-nan', '-nan'], ['0x00000000'], ['f32x4', 'f32x4', 'i32x4']],
            ["andnot", ['-nan', 'nan'], ['-0'], ['f32x4', 'f32x4', 'f32x4']],
            ["andnot", ['-nan', '-inf'], ['0x00400000'], ['f32x4', 'f32x4', 'i32x4']],
            ["andnot", ['-nan', 'inf'], ['0x80400000'], ['f32x4', 'f32x4', 'i32x4']],
            ["andnot", ['nan', 'nan'], ['0x00000000'], ['f32x4', 'f32x4', 'f32x4']],
            ["andnot", ['nan', '-inf'], ['0x00400000'], ['f32x4', 'f32x4', 'i32x4']],
            ["andnot", ['nan', 'inf'], ['0x00400000'], ['f32x4', 'f32x4', 'i32x4']],
            ["andnot", ['-inf', '-inf'], ['0x00000000'], ['f32x4', 'f32x4', 'f32x4']],
            ["andnot", ['-inf', 'inf'], ['0x80000000'], ['f32x4', 'f32x4', 'i32x4']],
            ["andnot", ['inf', 'inf'], ['0x00000000'], ['f32x4', 'f32x4', 'i32x4']]
        ]

    def gen_test_cases(self):
        """
        Generate test case file
        """
        with open('../simd_bitwise.wast', 'w+') as f_out:
            f_out.write(self.get_all_cases())


def gen_test_cases():
    """
    Generate test case file
    """
    bit_wise = SimdBitWise()
    bit_wise.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
