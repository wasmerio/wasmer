#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""
This python file is a tool class for test generation.
Currently only the 'AssertReturn' class that is used
to generate the 'assert_return' assertion.
TODO: Add more assertions
"""


# Generate assert_return to test
class AssertReturn:

    op = ''
    params = ''
    expected_result = ''

    def __init__(self, op, params, expected_result):

        # Convert to list if got str
        if isinstance(params, str):
            params = [params]
        if isinstance(expected_result, str):
            expected_result = [expected_result]

        self.op = op
        self.params = params
        self.expected_result = expected_result

    def __str__(self):
        assert_return = '(assert_return (invoke "{}"'.format(self.op)

        head_len = len(assert_return)

        # Add write space to make the test case easier to read
        params = []
        for param in self.params:
            white_space = ' '
            if len(params) != 0:
                white_space = '\n ' + ' ' * head_len
            params.append(white_space + param)

        results = []
        for result in self.expected_result:
            white_space = ' '
            if len(params) != 0 or len(results) != 0:
                white_space = '\n ' + ' ' * head_len
            results.append(white_space + result)

        return '{assert_head}{params}){expected_result})'.format(assert_head=assert_return, params=''.join(params), expected_result=''.join(results))


# Generate assert_invalid to test
class AssertInvalid:

    @staticmethod
    def get_arg_empty_test(op, extended_name, param_type, result_type, params):

        arg_empty_test = '(assert_invalid' \
                         '\n  (module' \
                         '\n    (func ${op}-{extended_name}{param_type}{result_type}' \
                         '\n      ({op}{params})' \
                         '\n    )' \
                         '\n  )' \
                         '\n  "type mismatch"' \
                         '\n)'

        def str_with_space(input_str):
            return (' ' if input_str else '') + input_str

        param_map = {
            'op': op,
            'extended_name': extended_name,
            'param_type': str_with_space(param_type),
            'result_type': str_with_space(result_type),
            'params': str_with_space(params),
        }

        return arg_empty_test.format(**param_map)


class AssertMalformed:
    """Generate an assert_malformed test"""

    @staticmethod
    def get_unknown_op_test(op, result_type, *params):
        malformed_template = '(assert_malformed (module quote "(memory 1) (func (result {result_type}) ({operator} {param}))") "unknown operator")'
        return malformed_template.format(
            operator=op, result_type=result_type, param=' '.join(params)
        )