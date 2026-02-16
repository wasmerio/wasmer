#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""
This class is used to generate common tests for SIMD comparison instructions.
Defines the test template to generate corresponding test file(simd_*_cmp.wast)
via using variable test data set and subclass from sub test template
"""

import abc
from simd import SIMD
from test_assert import AssertReturn, AssertInvalid


# Generate common comparison tests
class SimdCmpCase(object):

    __metaclass__ = abc.ABCMeta

    # Test case template
    CASE_TXT = """
;; Test all the {lane_type} comparison operators on major boundary values and all special values.

(module
  (func (export "eq") (param $x v128) (param $y v128) (result v128) ({lane_type}.eq (local.get $x) (local.get $y)))
  (func (export "ne") (param $x v128) (param $y v128) (result v128) ({lane_type}.ne (local.get $x) (local.get $y)))
  (func (export "lt_s") (param $x v128) (param $y v128) (result v128) ({lane_type}.lt_s (local.get $x) (local.get $y)))
  (func (export "lt_u") (param $x v128) (param $y v128) (result v128) ({lane_type}.lt_u (local.get $x) (local.get $y)))
  (func (export "le_s") (param $x v128) (param $y v128) (result v128) ({lane_type}.le_s (local.get $x) (local.get $y)))
  (func (export "le_u") (param $x v128) (param $y v128) (result v128) ({lane_type}.le_u (local.get $x) (local.get $y)))
  (func (export "gt_s") (param $x v128) (param $y v128) (result v128) ({lane_type}.gt_s (local.get $x) (local.get $y)))
  (func (export "gt_u") (param $x v128) (param $y v128) (result v128) ({lane_type}.gt_u (local.get $x) (local.get $y)))
  (func (export "ge_s") (param $x v128) (param $y v128) (result v128) ({lane_type}.ge_s (local.get $x) (local.get $y)))
  (func (export "ge_u") (param $x v128) (param $y v128) (result v128) ({lane_type}.ge_u (local.get $x) (local.get $y)))
)

{normal_case}


;; Type check

(assert_invalid (module (func (result v128) ({lane_type}.eq (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.ge_s (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.ge_u (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.gt_s (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.gt_u (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.le_s (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.le_u (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.lt_s (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.lt_u (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.ne (i32.const 0) (f32.const 0)))) "type mismatch")


;; combination

(module (memory 1)
  (func (export "eq-in-block")
    (block
      (drop
        (block (result v128)
          ({lane_type}.eq
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "ne-in-block")
    (block
      (drop
        (block (result v128)
          ({lane_type}.ne
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "lt_s-in-block")
    (block
      (drop
        (block (result v128)
          ({lane_type}.lt_s
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "le_u-in-block")
    (block
      (drop
        (block (result v128)
          ({lane_type}.le_u
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "gt_u-in-block")
    (block
      (drop
        (block (result v128)
          ({lane_type}.gt_u
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "ge_s-in-block")
    (block
      (drop
        (block (result v128)
          ({lane_type}.ge_s
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "nested-eq")
    (drop
      ({lane_type}.eq
        ({lane_type}.eq
          ({lane_type}.eq
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.eq
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        ({lane_type}.eq
          ({lane_type}.eq
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.eq
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-ne")
    (drop
      ({lane_type}.ne
        ({lane_type}.ne
          ({lane_type}.ne
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.ne
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        ({lane_type}.ne
          ({lane_type}.ne
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.ne
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-lt_s")
    (drop
      ({lane_type}.lt_s
        ({lane_type}.lt_s
          ({lane_type}.lt_s
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.lt_s
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        ({lane_type}.lt_s
          ({lane_type}.lt_s
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.lt_s
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-le_u")
    (drop
      ({lane_type}.le_u
        ({lane_type}.le_u
          ({lane_type}.le_u
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.le_u
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        ({lane_type}.le_u
          ({lane_type}.le_u
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.le_u
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-gt_u")
    (drop
      ({lane_type}.gt_u
        ({lane_type}.gt_u
          ({lane_type}.gt_u
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.gt_u
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        ({lane_type}.gt_u
          ({lane_type}.gt_u
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.gt_u
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-ge_s")
    (drop
      ({lane_type}.ge_s
        ({lane_type}.ge_s
          ({lane_type}.ge_s
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.ge_s
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        ({lane_type}.ge_s
          ({lane_type}.ge_s
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.ge_s
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "as-param")
    (drop
      ({lane_type}.ge_u
        ({lane_type}.eq
          ({lane_type}.lt_s
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.le_u
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        ({lane_type}.ne
          ({lane_type}.gt_s
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          ({lane_type}.lt_u
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
)
(assert_return (invoke "eq-in-block"))
(assert_return (invoke "ne-in-block"))
(assert_return (invoke "lt_s-in-block"))
(assert_return (invoke "le_u-in-block"))
(assert_return (invoke "gt_u-in-block"))
(assert_return (invoke "ge_s-in-block"))
(assert_return (invoke "nested-eq"))
(assert_return (invoke "nested-ne"))
(assert_return (invoke "nested-lt_s"))
(assert_return (invoke "nested-le_u"))
(assert_return (invoke "nested-gt_u"))
(assert_return (invoke "nested-ge_s"))
(assert_return (invoke "as-param"))

"""

    # lane type [e.g. i8x16, i16x8, i32x4, f32x4]
    LANE_TYPE = 'i8x16'

    def __init__(self):
        super(SimdCmpCase, self).__init__()

    def __str__(self):
        return self.get_all_cases()

    # This method requires subclass overloading with its own type of test data.
    @abc.abstractmethod
    def get_case_data(self):
        pass

    # Generate normal case with test datas
    def get_normal_case(self):

        s = SIMD()

        case_data = self.get_case_data()

        cases = []

        for item in case_data:
            # Recognize '#' as a commentary
            if item[0] == '#':
                cases.append('\n;; {}'.format(item[1]))
                continue

            """
            Generate assert_return
            Params: instruction: instruction name;
                    param: param for instruction;
                    ret: excepted result;
                    lane_type: lane type
            """
            instruction, param, ret, lane_type = item
            cases.append(str(AssertReturn(instruction,
                                          [s.v128_const(param[0], lane_type[0]),
                                           s.v128_const(param[1], lane_type[1])],
                                          s.v128_const(ret, lane_type[2]))))

        return '\n'.join(cases)

    def argument_empty_test(self):
        """Test cases with empty argument.
        """
        cases = []

        cases.append('\n;; Test operation with empty argument\n')

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

        return '\n'.join(cases)

    # Generate all test cases
    def get_all_cases(self):

        case_data = {'normal_case': self.get_normal_case(),
                     'lane_type': self.LANE_TYPE}

        # Generate tests using the test template
        return self.CASE_TXT.format(**case_data) + self.argument_empty_test()

    # Generate test case file
    def gen_test_cases(self):
        with open('../simd_{}_cmp.wast'.format(self.LANE_TYPE), 'w+') as f_out:
            f_out.write(self.get_all_cases())
            f_out.close()
