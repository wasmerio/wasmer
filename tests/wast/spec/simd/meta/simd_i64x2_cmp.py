#!/usr/bin/env python3

from simd_compare import SimdCmpCase


# Generate i64x2 test case
class Simdi64x2CmpCase(SimdCmpCase):
    LANE_TYPE = 'i64x2'

    BINARY_OPS = ['eq', 'ne']

    # Override this since i64x2 does not support as many comparison instructions.
    CASE_TXT = """
;; Test all the {lane_type} comparison operators on major boundary values and all special values.

(module
  (func (export "eq") (param $x v128) (param $y v128) (result v128) ({lane_type}.eq (local.get $x) (local.get $y)))
  (func (export "ne") (param $x v128) (param $y v128) (result v128) ({lane_type}.ne (local.get $x) (local.get $y)))
  (func (export "lt_s") (param $x v128) (param $y v128) (result v128) ({lane_type}.lt_s (local.get $x) (local.get $y)))
  (func (export "le_s") (param $x v128) (param $y v128) (result v128) ({lane_type}.le_s (local.get $x) (local.get $y)))
  (func (export "gt_s") (param $x v128) (param $y v128) (result v128) ({lane_type}.gt_s (local.get $x) (local.get $y)))
  (func (export "ge_s") (param $x v128) (param $y v128) (result v128) ({lane_type}.ge_s (local.get $x) (local.get $y)))
)

{normal_case}

;; Type check

(assert_invalid (module (func (result v128) ({lane_type}.eq (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.ne (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.ge_s (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.gt_s (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.le_s (i32.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result v128) ({lane_type}.lt_s (i32.const 0) (f32.const 0)))) "type mismatch")
"""

    def get_case_data(self):
        forms = ['i64x2'] * 3
        case_data = []

        case_data.append(['#', 'eq'])
        case_data.append(['#', 'i64x2.eq  (i64x2) (i64x2)'])
        case_data.append(['eq', ['0xFFFFFFFFFFFFFFFF', '0xFFFFFFFFFFFFFFFF'], '-1', forms])
        case_data.append(['eq', ['0x0000000000000000', '0x0000000000000000'], '-1', forms])
        case_data.append(['eq', ['0xF0F0F0F0F0F0F0F0', '0xF0F0F0F0F0F0F0F0'], '-1', forms])
        case_data.append(['eq', ['0x0F0F0F0F0F0F0F0F', '0x0F0F0F0F0F0F0F0F'], '-1', forms])
        case_data.append(['eq', [['0xFFFFFFFFFFFFFFFF', '0x0000000000000000'], ['0xFFFFFFFFFFFFFFFF', '0x0000000000000000']], '-1', forms])
        case_data.append(['eq', [['0x0000000000000000', '0xFFFFFFFFFFFFFFFF'], ['0x0000000000000000', '0xFFFFFFFFFFFFFFFF']], '-1', forms])
        case_data.append(['eq', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '-1', forms])
        case_data.append(['eq', ['0xFFFFFFFFFFFFFFFF', '0x0FFFFFFFFFFFFFFF'], '0', forms])
        case_data.append(['eq', ['0x1', '0x2'], '0', forms])

        case_data.append(['#', 'ne'])
        case_data.append(['#', 'i64x2.ne  (i64x2) (i64x2)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['ne', ['0xFFFFFFFFFFFFFFFF', '0xFFFFFFFFFFFFFFFF'], '0', forms])
        case_data.append(['ne', ['0x0000000000000000', '0x0000000000000000'], '0', forms])
        case_data.append(['ne', ['0xF0F0F0F0F0F0F0F0', '0xF0F0F0F0F0F0F0F0'], '0', forms])
        case_data.append(['ne', ['0x0F0F0F0F0F0F0F0F', '0x0F0F0F0F0F0F0F0F'], '0', forms])
        case_data.append(['ne', [['0xFFFFFFFFFFFFFFFF', '0x0000000000000000'], ['0xFFFFFFFFFFFFFFFF', '0x0000000000000000']], '0', forms])
        case_data.append(['ne', [['0x0000000000000000', '0xFFFFFFFFFFFFFFFF'], ['0x0000000000000000', '0xFFFFFFFFFFFFFFFF']], '0', forms])
        case_data.append(['ne', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '0', forms])

        # lt_s
        # i64x2.lt_s  (i64x2) (i64x2)
        case_data.append(['#', 'lt_s'])
        case_data.append(['#', 'i64x2.lt_s  (i64x2) (i64x2)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['lt_s', ['0xFFFFFFFFFFFFFFFF', '0xFFFFFFFFFFFFFFFF'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['0x0000000000000000', '0x0000000000000000'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['0xF0F0F0F0F0F0F0F0', '0xF0F0F0F0F0F0F0F0'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['0x0F0F0F0F0F0F0F0F', '0x0F0F0F0F0F0F0F0F'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', [['0xFFFFFFFFFFFFFFFF', '0x0000000000000000'], ['0xFFFFFFFFFFFFFFFF', '0x0000000000000000']], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', [['0x0000000000000000', '0xFFFFFFFFFFFFFFFF'], ['0x0000000000000000', '0xFFFFFFFFFFFFFFFF']], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', [['0x0302010011100904', '0x1A0B0A12FFABAA1B'],
                          ['0x0302010011100904', '0x1A0B0A12FFABAA1B']], '0', ['i64x2', 'i64x2', 'i64x2']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['lt_s', ['0xFFFFFFFFFFFFFFFF', '18446744073709551615'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['0xFFFFFFFFFFFFFFFF', '-1'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['0x8080808080808080', '9259542123273814144'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['0x8080808080808080', '-9187201950435737472'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', [['0x8382818000FFFEFD', '0x7F020100FFFEFD80'],
                          ['-8970465120996032771', '9151878496576798080']], '0', ['i64x2', 'i64x2', 'i64x2']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['lt_s', ['-1', '-1'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['0', '0'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['18446744073709551615', '18446744073709551615'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', ['18446744073709551615', '-1'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', [['18446744073709551615', '0'], ['18446744073709551615', '0']], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', [['0', '18446744073709551615'], ['0', '18446744073709551615']], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['lt_s', [['-9223372036854775807', '18446744073709551615'],
                          ['9223372036854775809', '-1']], '0', ['i64x2', 'i64x2', 'i64x2']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['lt_s', [['0xc060000000000000', '0xc05fc00000000000'],
                          ['-128.0', '-127.0']], '0', ['i64x2', 'f64x2', 'i64x2']])
        case_data.append(['lt_s', [['0x3ff0000000000000', '0x405fc00000000000'],
                          ['1.0', '127.0']], '0', ['i64x2', 'f64x2', 'i64x2']])

        # le_s
        # i64x2.le_s  (i64x2) (i64x2)
        case_data.append(['#', 'le_s'])
        case_data.append(['#', 'i64x2.le_s  (i64x2) (i64x2)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['le_s', ['0xFFFFFFFFFFFFFFFF', '0xFFFFFFFFFFFFFFFF'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['0x0000000000000000', '0x0000000000000000'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['0xF0F0F0F0F0F0F0F0', '0xF0F0F0F0F0F0F0F0'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['0x0F0F0F0F0F0F0F0F', '0x0F0F0F0F0F0F0F0F'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', [['0xFFFFFFFFFFFFFFFF', '0x0000000000000000'], ['0xFFFFFFFFFFFFFFFF', '0x0000000000000000']], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', [['0x0000000000000000', '0xFFFFFFFFFFFFFFFF'], ['0x0000000000000000', '0xFFFFFFFFFFFFFFFF']], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', [['0x0302010011100904', '0x1A0B0A12FFABAA1B'],
                          ['0x0302010011100904', '0x1A0B0A12FFABAA1B']], '-1', ['i64x2', 'i64x2', 'i64x2']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['le_s', ['0xFFFFFFFFFFFFFFFF', '18446744073709551615'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['0xFFFFFFFFFFFFFFFF', '-1'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['0x8080808080808080', '9259542123273814144'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['0x8080808080808080', '-9187201950435737472'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', [['0x8382818000FFFEFD', '0x7F020100FFFEFD80'],
                          ['-8970465120996032771', '9151878496576798080']], '-1', ['i64x2', 'i64x2', 'i64x2']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['le_s', ['-1', '-1'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', [['0', '0'], ['0', '-1']], ['-1', '0'], ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['0', '0'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['18446744073709551615', '18446744073709551615'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', ['18446744073709551615', '-1'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', [['18446744073709551615', '0'], ['18446744073709551615', '0']], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', [['0', '18446744073709551615'], ['0', '18446744073709551615']], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['le_s', [['-9223372036854775807', '18446744073709551615'],
                          ['9223372036854775809', '-1']], '-1', ['i64x2', 'i64x2', 'i64x2']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['le_s', [['0xc060000000000000', '0xc05fc00000000000'],
                          ['-128.0', '-127.0']], '-1', ['i64x2', 'f64x2', 'i64x2']])
        case_data.append(['le_s', [['0x3ff0000000000000', '0x405fc00000000000'],
                          ['1.0', '127.0']], '-1', ['i64x2', 'f64x2', 'i64x2']])

        # gt_s
        # i64x2.gt_s  (i64x2) (i64x2)
        case_data.append(['#', 'gt_s'])
        case_data.append(['#', 'i64x2.gt_s  (i64x2) (i64x2)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['gt_s', ['0xFFFFFFFFFFFFFFFF', '0xFFFFFFFFFFFFFFFF'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['0x0000000000000000', '0x0000000000000000'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['0xF0F0F0F0F0F0F0F0', '0xF0F0F0F0F0F0F0F0'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['0x0F0F0F0F0F0F0F0F', '0x0F0F0F0F0F0F0F0F'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', [['0xFFFFFFFFFFFFFFFF', '0x0000000000000000'], ['0xFFFFFFFFFFFFFFFF', '0x0000000000000000']], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', [['0x0000000000000000', '0xFFFFFFFFFFFFFFFF'], ['0x0000000000000000', '0xFFFFFFFFFFFFFFFF']], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', [['0x0302010011100904', '0x1A0B0A12FFABAA1B'],
                          ['0x0302010011100904', '0x1A0B0A12FFABAA1B']], '0', ['i64x2', 'i64x2', 'i64x2']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['gt_s', ['0xFFFFFFFFFFFFFFFF', '18446744073709551615'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['0xFFFFFFFFFFFFFFFF', '-1'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['0x8080808080808080', '9259542123273814144'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['0x8080808080808080', '-9187201950435737472'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', [['0x8382818000FFFEFD', '0x7F020100FFFEFD80'],
                          ['-8970465120996032771', '9151878496576798080']], '0', ['i64x2', 'i64x2', 'i64x2']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['gt_s', ['-1', '-1'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['0', '0'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['18446744073709551615', '18446744073709551615'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', ['18446744073709551615', '-1'], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', [['18446744073709551615', '0'], ['18446744073709551615', '0']], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', [['0', '18446744073709551615'], ['0', '18446744073709551615']], '0', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['gt_s', [['-9223372036854775807', '18446744073709551615'],
                          ['9223372036854775809', '-1']], '0', ['i64x2', 'i64x2', 'i64x2']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['gt_s', [['0xc060000000000000', '0xc05fc00000000000'],
                          ['-128.0', '-127.0']], '0', ['i64x2', 'f64x2', 'i64x2']])
        case_data.append(['gt_s', [['0x3ff0000000000000', '0x405fc00000000000'],
                          ['1.0', '127.0']], '0', ['i64x2', 'f64x2', 'i64x2']])

        # ge_s
        # i64x2.ge_s  (i64x2) (i64x2)
        case_data.append(['#', 'ge_s'])
        case_data.append(['#', 'i64x2.ge_s  (i64x2) (i64x2)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['ge_s', ['0xFFFFFFFFFFFFFFFF', '0xFFFFFFFFFFFFFFFF'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['0x0000000000000000', '0x0000000000000000'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['0xF0F0F0F0F0F0F0F0', '0xF0F0F0F0F0F0F0F0'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['0x0F0F0F0F0F0F0F0F', '0x0F0F0F0F0F0F0F0F'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', [['0xFFFFFFFFFFFFFFFF', '0x0000000000000000'], ['0xFFFFFFFFFFFFFFFF', '0x0000000000000000']], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', [['0x0000000000000000', '0xFFFFFFFFFFFFFFFF'], ['0x0000000000000000', '0xFFFFFFFFFFFFFFFF']], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', [['0x0302010011100904', '0x1A0B0A12FFABAA1B'],
                          ['0x0302010011100904', '0x1A0B0A12FFABAA1B']], '-1', ['i64x2', 'i64x2', 'i64x2']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['ge_s', ['0xFFFFFFFFFFFFFFFF', '18446744073709551615'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['0xFFFFFFFFFFFFFFFF', '-1'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['0x8080808080808080', '9259542123273814144'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['0x8080808080808080', '-9187201950435737472'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', [['0x8382818000FFFEFD', '0x7F020100FFFEFD80'],
                          ['-8970465120996032771', '9151878496576798080']], '-1', ['i64x2', 'i64x2', 'i64x2']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['ge_s', ['-1', '-1'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', [['-1', '-1'], ['0', '-1']], ['0', '-1'], ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['0', '0'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['18446744073709551615', '18446744073709551615'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', ['18446744073709551615', '-1'], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', [['18446744073709551615', '0'], ['18446744073709551615', '0']], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', [['0', '18446744073709551615'], ['0', '18446744073709551615']], '-1', ['i64x2', 'i64x2', 'i64x2']])
        case_data.append(['ge_s', [['-9223372036854775807', '18446744073709551615'],
                          ['9223372036854775809', '-1']], '-1', ['i64x2', 'i64x2', 'i64x2']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['ge_s', [['0xc060000000000000', '0xc05fc00000000000'],
                          ['-128.0', '-127.0']], '-1', ['i64x2', 'f64x2', 'i64x2']])
        case_data.append(['ge_s', [['0x3ff0000000000000', '0x405fc00000000000'],
                          ['1.0', '127.0']], '-1', ['i64x2', 'f64x2', 'i64x2']])

        return case_data


def gen_test_cases():
    i64x2 = Simdi64x2CmpCase()
    i64x2.gen_test_cases()


if __name__ == '__main__':
    i64x2 = Simdi64x2CmpCase()
    i64x2.gen_test_cases()
