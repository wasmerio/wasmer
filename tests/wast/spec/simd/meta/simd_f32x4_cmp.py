#!/usr/bin/env python3

"""
This file is used for generating simd_f32x4_cmp.wast file.
Which inherites from `SimdCmpCase` class, overloads
the `get_test_cases` method, and reset the Test Case template.
The reason why this is different from other cmp files is that
f32x4 only has 6 comparison instructions but with amounts of
test datas.
"""
import struct
from simd_compare import SimdCmpCase


# Generate f32x4 test case
class Simdf32x4CmpCase(SimdCmpCase):

    LANE_TYPE = 'f32x4'

    BINARY_OPS = ['eq', 'ne', 'lt', 'le', 'gt', 'ge']

    # Test template, using this template to generate tests with variable test datas.
    CASE_TXT = """;; Test all the {lane_type} comparison operators on major boundary values and all special values.

(module
  (func (export "eq") (param $x v128) (param $y v128) (result v128) (f32x4.eq (local.get $x) (local.get $y)))
  (func (export "ne") (param $x v128) (param $y v128) (result v128) (f32x4.ne (local.get $x) (local.get $y)))
  (func (export "lt") (param $x v128) (param $y v128) (result v128) (f32x4.lt (local.get $x) (local.get $y)))
  (func (export "le") (param $x v128) (param $y v128) (result v128) (f32x4.le (local.get $x) (local.get $y)))
  (func (export "gt") (param $x v128) (param $y v128) (result v128) (f32x4.gt (local.get $x) (local.get $y)))
  (func (export "ge") (param $x v128) (param $y v128) (result v128) (f32x4.ge (local.get $x) (local.get $y)))
)
{normal_case}


;; Type check

(assert_invalid (module (func (result v128) (f32x4.eq (i64.const 0) (f64.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) (f32x4.ge (i64.const 0) (f64.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) (f32x4.gt (i64.const 0) (f64.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) (f32x4.le (i64.const 0) (f64.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) (f32x4.lt (i64.const 0) (f64.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) (f32x4.ne (i64.const 0) (f64.const 0)))) "type mismatch")


;; Unknown operators

(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (f4x32.eq (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (f4x32.ge (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (f4x32.gt (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (f4x32.le (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (f4x32.lt (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (f4x32.ne (local.get $x) (local.get $y)))") "unknown operator")


;; Combination

(module (memory 1)
  (func (export "eq-in-block")
    (block
      (drop
        (block (result v128)
          (f32x4.eq
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
          (f32x4.ne
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "lt-in-block")
    (block
      (drop
        (block (result v128)
          (f32x4.lt
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "le-in-block")
    (block
      (drop
        (block (result v128)
          (f32x4.le
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "gt-in-block")
    (block
      (drop
        (block (result v128)
          (f32x4.gt
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "ge-in-block")
    (block
      (drop
        (block (result v128)
          (f32x4.ge
            (block (result v128) (v128.load (i32.const 0)))
            (block (result v128) (v128.load (i32.const 1)))
          )
        )
      )
    )
  )
  (func (export "nested-eq")
    (drop
      (f32x4.eq
        (f32x4.eq
          (f32x4.eq
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.eq
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        (f32x4.eq
          (f32x4.eq
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.eq
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-ne")
    (drop
      (f32x4.ne
        (f32x4.ne
          (f32x4.ne
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.ne
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        (f32x4.ne
          (f32x4.ne
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.ne
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-lt")
    (drop
      (f32x4.lt
        (f32x4.lt
          (f32x4.lt
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.lt
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        (f32x4.lt
          (f32x4.lt
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.lt
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-le")
    (drop
      (f32x4.le
        (f32x4.le
          (f32x4.le
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.le
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        (f32x4.le
          (f32x4.le
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.le
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-gt")
    (drop
      (f32x4.gt
        (f32x4.gt
          (f32x4.gt
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.gt
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        (f32x4.gt
          (f32x4.gt
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.gt
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "nested-ge")
    (drop
      (f32x4.ge
        (f32x4.ge
          (f32x4.ge
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.ge
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        (f32x4.ge
          (f32x4.ge
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.ge
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
      )
    )
  )
  (func (export "as-param")
    (drop
      (f32x4.ge
        (f32x4.eq
          (f32x4.lt
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.le
            (v128.load (i32.const 2))
            (v128.load (i32.const 3))
          )
        )
        (f32x4.ne
          (f32x4.gt
            (v128.load (i32.const 0))
            (v128.load (i32.const 1))
          )
          (f32x4.lt
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
(assert_return (invoke "lt-in-block"))
(assert_return (invoke "le-in-block"))
(assert_return (invoke "gt-in-block"))
(assert_return (invoke "ge-in-block"))
(assert_return (invoke "nested-eq"))
(assert_return (invoke "nested-ne"))
(assert_return (invoke "nested-lt"))
(assert_return (invoke "nested-le"))
(assert_return (invoke "nested-gt"))
(assert_return (invoke "nested-ge"))
(assert_return (invoke "as-param"))
"""

    # Overloads base class method and sets test data for f32x4.
    def get_case_data(self):

        case_data = []

        operand1 = ('nan', '0x1p-149', '-nan:0x200000', '-inf', '0x1.921fb6p+2',
                    '0x1p+0', '-0x1.fffffep+127', '-0x0p+0', '-0x1p-1', '0x1.fffffep+127',
                    '-nan', '-0x1p-149', '-0x1p-126', '0x1p-1', '-0x1.921fb6p+2',
                    'nan:0x200000', '0x0p+0', 'inf', '-0x1p+0', '0x1p-126')
        operand2 = ('nan', '0x1p-149', '-nan:0x200000', '-inf', '0x1.921fb6p+2',
                    '0x1p+0', '-0x1.fffffep+127', '-0x0p+0', '-0x1p-1', '0x1.fffffep+127',
                    '-nan', '-0x1p-149', '-0x1p-126', '0x1p-1', '-0x1.921fb6p+2',
                    'nan:0x200000', '0x0p+0', 'inf', '-0x1p+0', '0x1p-126')
        LITERAL_NUMBERS = (
            '0123456789e019', '0123456789e-019',
            '0123456789.e019', '0123456789.e+019',
            '0123456789.0123456789')
        Ops = ('eq', 'ne', 'lt', 'le', 'gt', 'ge')

        # Combinations between operand1 and operand2
        for op in Ops:
            case_data.append(['#', op])
            for param1 in operand1:
                for param2 in operand2:
                    case_data.append([op, [param1, param2], self.operate(op, param1, param2), ['f32x4', 'f32x4', 'i32x4']])

            for param1 in LITERAL_NUMBERS:
                for param2 in LITERAL_NUMBERS:
                    case_data.append([op, [param1, param2], self.operate(op, param1, param2), ['f32x4', 'f32x4', 'i32x4']])
        # eq
        case_data.append(['#', 'eq'])

        # f32x4.eq  (f32x4) (i8x16)
        case_data.append(['#', 'f32x4.eq  (f32x4) (i8x16)'])
        case_data.append(['eq', [['-1', '0', '1', '2.0'], ['-1', '-1', '-1', '-1', '0', '0', '0', '0', '1', '1', '1', '1', '2', '2', '2']], ['0', '-1', '0', '0'], ['f32x4', 'i8x16', 'i32x4']])

        # f32x4.eq  (f32x4) (i16x8)
        case_data.append(['#', 'f32x4.eq  (f32x4) (i16x8)'])
        case_data.append(['eq', [['-1', '0', '1', '2.0'], ['-1', '-1', '0', '0', '1', '1', '2']], ['0', '-1', '0', '0'], ['f32x4', 'i16x8', 'i32x4']])

        # f32x4.eq  (f32x4) (i32x4)
        case_data.append(['#', 'f32x4.eq  (f32x4) (i32x4)'])
        case_data.append(['eq', [['-1', '0', '1', '2.0'], ['3212836864', '0', '1', '2']], ['-1 -1', '0', '0', ''], ['f32x4', 'i32x4', 'i32x4']])

        # ne
        case_data.append(['#', 'ne'])

        # f32x4.ne  (f32x4) (i8x16)
        case_data.append(['#', 'f32x4.ne  (f32x4) (i8x16)'])
        case_data.append(['ne', [['-1', '0', '1', '2.0'], ['-1', '-1', '-1', '-1', '0', '0', '0', '0', '1', '1', '1', '1', '2', '2', '2']], ['-1', '0', '-1', '-1'], ['f32x4', 'i8x16', 'i32x4']])

        # f32x4.ne  (f32x4) (i16x8)
        case_data.append(['#', 'f32x4.ne  (f32x4) (i16x8)'])
        case_data.append(['ne', [['-1', '0', '1', '2.0'], ['-1', '-1', '0', '0', '1', '1', '2']], ['-1', '0', '-1', '-1'], ['f32x4', 'i16x8', 'i32x4']])

        # f32x4.ne  (f32x4) (i32x4)
        case_data.append(['#', 'f32x4.ne  (f32x4) (i32x4)'])
        case_data.append(['ne', [['-1', '0', '1', '2.0'], ['3212836864', '0', '1', '2']], ['0', '0', '-1', '-1'], ['f32x4', 'i32x4', 'i32x4']])

        # lt
        case_data.append(['#', 'lt'])

        # f32x4.lt  (f32x4) (i8x16)
        case_data.append(['#', 'f32x4.lt  (f32x4) (i8x16)'])
        case_data.append(['lt', [['-1', '0', '1', '2.0'], ['-1', '-1', '-1', '-1', '0', '0', '0', '0', '1', '1', '1', '1', '2', '2', '2']], ['0', '0', '0', '0'], ['f32x4', 'i8x16', 'i32x4']])

        # f32x4.lt  (f32x4) (i16x8)
        case_data.append(['#', 'f32x4.lt  (f32x4) (i16x8)'])
        case_data.append(['lt', [['-1', '0', '1', '2.0'], ['-1', '-1', '0', '0', '1', '1', '2']], ['0', '0', '0', '0'], ['f32x4', 'i16x8', 'i32x4']])

        # f32x4.lt  (f32x4) (i32x4)
        case_data.append(['#', 'f32x4.lt  (f32x4) (i32x4)'])
        case_data.append(['lt', [['-1', '0', '1', '2.0'], ['3212836864', '0', '1', '2']], ['0', '0', '0', '0'], ['f32x4', 'i32x4', 'i32x4']])

        # le
        case_data.append(['#', 'le'])

        # f32x4.le  (f32x4) (i8x16)
        case_data.append(['#', 'f32x4.le  (f32x4) (i8x16)'])
        case_data.append(['le', [['-1', '0', '1', '2.0'], ['-1', '-1', '-1', '-1', '0', '0', '0', '0', '1', '1', '1', '1', '2', '2', '2']], ['0', '-1', '0', '0'], ['f32x4', 'i8x16', 'i32x4']])

        # f32x4.le  (f32x4) (i16x8)
        case_data.append(['#', 'f32x4.le  (f32x4) (i16x8)'])
        case_data.append(['le', [['-1', '0', '1', '2.0'], ['-1', '-1', '0', '0', '1', '1', '2']], ['0', '-1', '0', '0'], ['f32x4', 'i16x8', 'i32x4']])

        # f32x4.le  (f32x4) (i32x4)
        case_data.append(['#', 'f32x4.le  (f32x4) (i32x4)'])
        case_data.append(['le', [['-1', '0', '1', '2.0'], ['3212836864', '0', '1', '2']], ['-1', '-1', '0', '0'], ['f32x4', 'i32x4', 'i32x4']])

        # gt
        case_data.append(['#', 'gt'])

        # f32x4.gt  (f32x4) (i8x16)
        case_data.append(['#', 'f32x4.gt  (f32x4) (i8x16)'])
        case_data.append(['gt', [['-1', '0', '1', '2.0'], ['-1', '-1', '-1', '-1', '0', '0', '0', '0', '1', '1', '1', '1', '2', '2', '2']], ['0', '0', '-1', '-1'], ['f32x4', 'i8x16', 'i32x4']])

        # f32x4.gt  (f32x4) (i16x8)
        case_data.append(['#', 'f32x4.gt  (f32x4) (i16x8)'])
        case_data.append(['gt', [['-1', '0', '1', '2.0'], ['-1', '-1', '0', '0', '1', '1', '2']], ['0', '0', '-1', '-1'], ['f32x4', 'i16x8', 'i32x4']])

        # f32x4.gt  (f32x4) (i32x4)
        case_data.append(['#', 'f32x4.gt  (f32x4) (i32x4)'])
        case_data.append(['gt', [['-1', '0', '1', '2.0'], ['3212836864', '0', '1', '2']], ['0', '0', '-1', '-1'], ['f32x4', 'i32x4', 'i32x4']])

        # ge
        case_data.append(['#', 'ge'])

        # f32x4.ge  (f32x4) (i8x16)
        case_data.append(['#', 'f32x4.ge  (f32x4) (i8x16)'])
        case_data.append(['ge', [['-1', '0', '1', '2.0'], ['-1', '-1', '-1', '-1', '0', '0', '0', '0', '1', '1', '1', '1', '2', '2', '2']], ['0', '-1', '-1', '-1'], ['f32x4', 'i8x16', 'i32x4']])

        # f32x4.ge  (f32x4) (i16x8)
        case_data.append(['#', 'f32x4.ge  (f32x4) (i16x8)'])
        case_data.append(['ge', [['-1', '0', '1', '2.0'], ['-1', '-1', '0', '0', '1', '1', '2']], ['0', '-1', '-1', '-1'], ['f32x4', 'i16x8', 'i32x4']])

        # f32x4.ge  (f32x4) (i32x4)
        case_data.append(['#', 'f32x4.ge  (f32x4) (i32x4)'])
        case_data.append(['ge', [['-1', '0', '1', '2.0'], ['3212836864', '0', '1', '2']], ['-1', '-1', '-1', '-1'], ['f32x4', 'i32x4', 'i32x4']])

        return case_data

    def special_float2dec(self, p):
        if p in ('0x0p+0', '-0x0p+0'):
            return 0.0
        if p == 'inf':
            return float(340282366920938463463374607431768211456)
        if p == '-inf':
            return -float(340282366920938463463374607431768211456)

        if '0x' in p:
            f = float.fromhex(p)
        else:
            f = float(p)

        return struct.unpack('f', struct.pack('f', f))[0]

    def operate(self, op, p1, p2):
        for p in (p1, p2):
            if 'nan' in p:
                if op == 'ne':
                    return '-1'
                else:
                    return '0'

        num1 = self.special_float2dec(p1)
        num2 = self.special_float2dec(p2)

        if op == 'eq':
            if num1 == num2:
                return '-1'

        if op == 'ne':
            if num1 != num2:
                return '-1'
        if op == 'lt':
            if num1 < num2:
                return '-1'
        if op == 'le':
            if num1 <= num2:
                return '-1'
        if op == 'gt':
            if num1 > num2:
                return '-1'
        if op == 'ge':
            if num1 >= num2:
                return '-1'

        return '0'


def gen_test_cases():
    f32x4 = Simdf32x4CmpCase()
    f32x4.gen_test_cases()


if __name__ == '__main__':
    f32x4 = Simdf32x4CmpCase()
    f32x4.gen_test_cases()
