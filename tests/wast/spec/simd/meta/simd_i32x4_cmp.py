#!/usr/bin/env python3

"""
This file is used for generating i32x4 related test cases
which inherites from the 'SimdCmpCase' class and overloads
with the 'get_test_cases' method.
"""

from simd_compare import SimdCmpCase


# Generate i32x4 test case
class Simdi32x4CmpCase(SimdCmpCase):

    LANE_TYPE = 'i32x4'

    BINARY_OPS = ['eq', 'ne', 'lt_s', 'lt_u', 'le_s', 'le_u', 'gt_s', 'gt_u', 'ge_s', 'ge_u']

    # Overload base class method and set test data for i32x4.
    def get_case_data(self):

        case_data = []

        # eq
        # i32x4.eq  (i32x4) (i32x4)
        case_data.append(['#', 'eq'])
        case_data.append(['#', 'i32x4.eq  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['eq', ['0xFFFFFFFF', '0xFFFFFFFF'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['0x00000000', '0x00000000'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['0xF0F0F0F0', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['0x0F0F0F0F', '0x0F0F0F0F'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['eq', ['0xFFFFFFFF', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['0xFFFFFFFF', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['0x80808080', '2155905152'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['0x80808080', '-2139062144'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['eq', ['-1', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['0', '0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['4294967295', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['4294967295', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['4294967295', '0'], ['4294967295', '0']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['0', '4294967295'], ['0', '4294967295']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['-2147483647', '4294967295', '0', '-1'], ['2147483649', '-1', '0', '-1']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['eq', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'], ['-128.0', '-127.0', '-1.0', '0.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['eq', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'], ['1.0', '127.0', '128.0', '255.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['eq', ['0x0F0F0F0F', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['-1', '0', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.eq  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.eq  (i32x4) (i8x16)'])
        case_data.append(['eq', ['0xFFFFFFFF', '0xFF'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['eq', ['4294967295', '255'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['eq', ['0', '0'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['eq', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['eq', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['eq', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['0', '-1', '0', '-1'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['eq', ['0x55555555', '0xAA'], '0', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.eq  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.eq  (i32x4) (i16x8)'])
        case_data.append(['eq', ['0xFFFFFFFF', '0xFFFF'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['eq', ['4294967295', '65535'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['eq', ['0', '0'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['eq', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['eq', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['eq', [['4294967295', '0', '1', '65535'], ['65535', '65535', '0', '0', '1', '0', '65535', '65535']], ['-1', '-1', '-1', '0'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['eq', ['0x55555555', '0xAAAA'], '0', ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['eq', ['0_123_456_789', '123456789'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['eq', ['0x0_1234_5678', '0x12345678'], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # ne
        # i32x4.ne  (i32x4) (i32x4)
        case_data.append(['#', 'ne'])
        case_data.append(['#', 'i32x4.ne  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['ne', ['0xFFFFFFFF', '0xFFFFFFFF'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['0x00000000', '0x00000000'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['0xF0F0F0F0', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['0x0F0F0F0F', '0x0F0F0F0F'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['ne', ['0xFFFFFFFF', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['0xFFFFFFFF', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['0x80808080', '2155905152'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['0x80808080', '-2139062144'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['ne', ['-1', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['0', '0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['4294967295', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['4294967295', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['4294967295', '0'], ['4294967295', '0']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['0', '4294967295'], ['0', '4294967295']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['-2147483647', '4294967295', '0', '-1'], ['2147483649', '-1', '0', '-1']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['ne', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'],
                          ['-128.0', '-127.0', '-1.0', '0.0']], '0', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['ne', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'],
                          ['1.0', '127.0', '128.0', '255.0']], '0', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['ne', ['0x0F0F0F0F', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['0', '-1', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.ne  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.ne  (i32x4) (i8x16)'])
        case_data.append(['ne', ['0xFFFFFFFF', '0xFF'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ne', ['4294967295', '255'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ne', ['0', '0'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ne', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ne', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ne', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['-1', '0', '-1', '0'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ne', ['0x55555555', '0xAA'], '-1', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.ne  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.ne  (i32x4) (i16x8)'])
        case_data.append(['ne', ['0xFFFFFFFF', '0xFFFF'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ne', ['4294967295', '65535'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ne', ['0', '0'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ne', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ne', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ne', [['-128', '0', '1', '255'], ['-128', '0', '1', '255']], ['-1', '0', '-1', '-1'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ne', ['0xAAAAAAAA', '0x5555'], ['-1', '-1', '-1', '-1'], ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['ne', ['0_123_456_789', '123456789'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ne', ['0x0_1234_5678', '0x12345678'], '0', ['i32x4', 'i32x4', 'i32x4']])

        # lt_s
        # i32x4.lt_s  (i32x4) (i32x4)
        case_data.append(['#', 'lt_s'])
        case_data.append(['#', 'i32x4.lt_s  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['lt_s', ['0xFFFFFFFF', '0xFFFFFFFF'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['0x00000000', '0x00000000'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['0xF0F0F0F0', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['0x0F0F0F0F', '0x0F0F0F0F'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['lt_s', ['0xFFFFFFFF', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['0xFFFFFFFF', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['0x80808080', '2155905152'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['0x80808080', '-2139062144'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['lt_s', ['-1', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['0', '0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['4294967295', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['4294967295', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['4294967295', '0'], ['4294967295', '0']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['0', '4294967295'], ['0', '4294967295']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['-2147483647', '4294967295', '0', '-1'],
                          ['2147483649', '-1', '0', '-1']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['lt_s', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'],
                          ['-128.0', '-127.0', '-1.0', '0.0']], '0', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['lt_s', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'],
                          ['1.0', '127.0', '128.0', '255.0']], '0', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['lt_s', ['0x0F0F0F0F', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], ['0', '0', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], ['0', '0', '0', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], ['-1', '-1', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['0', '0', '0', '-1'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.lt_s  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.lt_s  (i32x4) (i8x16)'])
        case_data.append(['lt_s', ['0xFFFFFFFF', '0xFF'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_s', ['4294967295', '255'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_s', ['0', '0'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_s', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_s', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_s', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['0', '0', '-1', '0'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_s', ['0x55555555', '0xAA'], '0', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.lt_s  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.lt_s  (i32x4) (i16x8)'])
        case_data.append(['lt_s', ['0xFFFFFFFF', '0xFFFF'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_s', ['4294967295', '65535'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_s', ['0', '0'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_s', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_s', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_s', [['-128', '0', '1', '255'], ['-128', '0', '1', '255']], ['0', '0', '-1', '-1'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_s', ['0xAAAAAAAA', '0x5555'], '-1', ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['lt_s', ['0_123_456_789', '123456789'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_s', ['0x0_90AB_cdef', '-0x6f543210'], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # lt_u
        # i32x4.lt_u  (i32x4) (i32x4)
        case_data.append(['#', 'lt_u'])
        case_data.append(['#', 'i32x4.lt_u  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['lt_u', ['0xFFFFFFFF', '0xFFFFFFFF'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['0x00000000', '0x00000000'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['0xF0F0F0F0', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['0x0F0F0F0F', '0x0F0F0F0F'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['lt_u', ['0xFFFFFFFF', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['0xFFFFFFFF', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['0x80808080', '2155905152'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['0x80808080', '-2139062144'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['lt_u', ['-1', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['0', '0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['4294967295', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['4294967295', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['4294967295', '0'], ['4294967295', '0']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['0', '4294967295'], ['0', '4294967295']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['-2147483647', '4294967295', '0', '-1'], ['2147483649', '-1', '0', '-1']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['lt_u', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'], ['-128.0', '-127.0', '-1.0', '0.0']], '0', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['lt_u', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'], ['1.0', '127.0', '128.0', '255.0']], '0', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['lt_u', ['0x0F0F0F0F', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], ['-1', '-1', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], ['-1', '0', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], ['-1', '-1', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['0', '-1', '-1', '0'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.lt_u  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.lt_u  (i32x4) (i8x16)'])
        case_data.append(['lt_u', ['0xFFFFFFFF', '0xFF'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_u', ['4294967295', '255'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_u', ['0', '0'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_u', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_u', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_u', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['0', '0', '-1', '0'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['lt_u', ['0x55555555', '0xAA'], '-1', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.lt_u  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.lt_u  (i32x4) (i16x8)'])
        case_data.append(['lt_u', ['0xFFFFFFFF', '0xFFFF'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_u', ['4294967295', '65535'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_u', ['0', '0'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_u', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_u', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_u', [['-128', '0', '1', '255'], ['-128', '0', '1', '255']], ['0', '0', '-1', '-1'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['lt_u', ['0xAAAAAAAA', '0x5555'], '0', ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['lt_u', ['0_123_456_789', '123456789'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['lt_u', ['0x0_90AB_cdef', '-0x6f543210'], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # le_s
        # i32x4.le_s  (i32x4) (i32x4)
        case_data.append(['#', 'le_s'])

        case_data.append(['#', 'i32x4.le_s  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['le_s', ['0xFFFFFFFF', '0xFFFFFFFF'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['0x00000000', '0x00000000'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['0xF0F0F0F0', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['0x0F0F0F0F', '0x0F0F0F0F'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['le_s', ['0xFFFFFFFF', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['0xFFFFFFFF', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['0x80808080', '2155905152'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['0x80808080', '-2139062144'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['le_s', ['-1', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['0', '0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['4294967295', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['4294967295', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['4294967295', '0'], ['4294967295', '0']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['0', '4294967295'], ['0', '4294967295']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['-2147483647', '4294967295', '0', '-1'],
                          ['2147483649', '-1', '0', '-1']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['le_s', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'], ['-128.0', '-127.0', '-1.0', '0.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['le_s', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'], ['1.0', '127.0', '128.0', '255.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['le_s', ['0x0F0F0F0F', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], ['0', '0', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], ['0', '0', '0', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], ['-1', '-1', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['-1', '0', '0', '-1'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.le_s  (i32x4)(i8x16)
        case_data.append(['#', 'i32x4.le_s  (i32x4)(i8x16)'])
        case_data.append(['le_s', ['0xFFFFFFFF', '0xFF'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_s', ['4294967295', '255'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_s', ['0', '0'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_s', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_s', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_s', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['0', '-1', '-1', '-1'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_s', ['0x55555555', '0xAA'], '0', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.le_s  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.le_s  (i32x4) (i16x8)'])
        case_data.append(['le_s', ['0xFFFFFFFF', '0xFFFF'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_s', ['4294967295', '65535'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_s', ['0', '0'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_s', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_s', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_s', [['-128', '0', '1', '255'], ['-128', '0', '1', '255']], ['0', '-1', '-1', '-1'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_s', ['0xAAAAAAAA', '0x5555'], '-1', ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['le_s', ['0_123_456_789', '123456789'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_s', ['0x0_1234_5678', '0x12345678'], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # le_u
        # i32x4.le_u  (i32x4) (i32x4)
        case_data.append(['#', 'le_u'])

        case_data.append(['#', 'i32x4.le_u  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['le_u', ['0xFFFFFFFF', '0xFFFFFFFF'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['0x00000000', '0x00000000'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['0xF0F0F0F0', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['0x0F0F0F0F', '0x0F0F0F0F'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['le_u', ['0xFFFFFFFF', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['0xFFFFFFFF', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['0x80808080', '2155905152'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['0x80808080', '-2139062144'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['le_u', ['-1', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['0', '0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['4294967295', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['4294967295', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['4294967295', '0'], ['4294967295', '0']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['0', '4294967295'], ['0', '4294967295']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['-2147483647', '4294967295', '0', '-1'], ['2147483649', '-1', '0', '-1']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['le_u', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'], ['-128.0', '-127.0', '-1.0', '0.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['le_u', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'], ['1.0', '127.0', '128.0', '255.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['le_u', ['0x0F0F0F0F', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], ['-1', '-1', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], ['-1', '0', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], ['-1', '-1', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['-1', '-1', '-1', '0'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.le_u  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.le_u  (i32x4) (i8x16)'])
        case_data.append(['le_u', ['0xFFFFFFFF', '0xFF'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_u', ['4294967295', '255'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_u', ['0', '0'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_u', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_u', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_u', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['0', '-1', '-1', '-1'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['le_u', ['0x55555555', '0xAA'], '-1', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.le_u  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.le_u  (i32x4) (i16x8)'])
        case_data.append(['le_u', ['0xFFFFFFFF', '0xFFFF'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_u', ['4294967295', '65535'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_u', ['0', '0'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_u', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_u', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_u', [['-128', '0', '1', '255'], ['-128', '0', '1', '255']], ['0', '-1', '-1', '-1'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['le_u', ['0xAAAAAAAA', '0x5555'], '0', ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['le_u', ['0_123_456_789', '123456789'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['le_u', ['0x0_90AB_cdef', '0x90ABcdef'], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # gt_s
        # i32x4.gt_s  (i32x4) (i32x4)
        case_data.append(['#', 'gt_s'])

        case_data.append(['#', 'i32x4.gt_s  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['gt_s', ['0xFFFFFFFF', '0xFFFFFFFF'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['0x00000000', '0x00000000'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['0xF0F0F0F0', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['0x0F0F0F0F', '0x0F0F0F0F'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['gt_s', ['0xFFFFFFFF', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['0xFFFFFFFF', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['0x80808080', '2155905152'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['0x80808080', '-2139062144'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['gt_s', ['-1', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['0', '0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['4294967295', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['4294967295', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['4294967295', '0'], ['4294967295', '0']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['0', '4294967295'], ['0', '4294967295']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['-2147483647', '4294967295', '0', '-1'], ['2147483649', '-1', '0', '-1']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['gt_s', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'], ['-128.0', '-127.0', '-1.0', '0.0']], '0', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['gt_s', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'], ['1.0', '127.0', '128.0', '255.0']], '0', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['gt_s', ['0x0F0F0F0F', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], ['-1', '-1', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], ['-1', '-1', '-1', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], ['0', '0', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['0', '-1', '-1', '0'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.gt_s  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.gt_s  (i32x4) (i8x16)'])
        case_data.append(['gt_s', ['0xFFFFFFFF', '0xFF'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_s', ['4294967295', '255'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_s', ['0', '0'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_s', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_s', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_s', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['-1', '0', '0', '0'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_s', ['0x55555555', '0xAA'], '-1', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.gt_s  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.gt_s  (i32x4) (i16x8)'])
        case_data.append(['gt_s', ['0xFFFFFFFF', '0xFFFF'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_s', ['4294967295', '65535'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_s', ['0', '0'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_s', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_s', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_s', [['65535', '0', '1', '32768'], ['65535', '65535', '0', '0', '1', '1', '32768', '32768']], ['-1', '0', '0', '-1'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_s', ['0xAAAAAAAA', '0x5555'], '0', ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['gt_s', ['0_123_456_789', '123456789'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_s', ['0x0_90AB_cdef', '-0x6f543211'], '0', ['i32x4', 'i32x4', 'i32x4']])

        # gt_u
        # i32x4.gt_u  (i32x4) (i32x4)
        case_data.append(['#', 'gt_u'])

        case_data.append(['#', 'i32x4.gt_u  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['gt_u', ['0xFFFFFFFF', '0xFFFFFFFF'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['0x00000000', '0x00000000'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['0xF0F0F0F0', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['0x0F0F0F0F', '0x0F0F0F0F'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['gt_u', ['0xFFFFFFFF', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['0xFFFFFFFF', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['0x80808080', '2155905152'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['0x80808080', '-2139062144'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['gt_u', ['-1', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['0', '0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['4294967295', '4294967295'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['4294967295', '-1'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['4294967295', '0'], ['4294967295', '0']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['0', '4294967295'], ['0', '4294967295']], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['-2147483647', '4294967295', '0', '-1'], ['2147483649', '-1', '0', '-1']], '0', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['gt_u', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'], ['-128.0', '-127.0', '-1.0', '0.0']], '0', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['gt_u', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'], ['1.0', '127.0', '128.0', '255.0']], '0', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['gt_u', ['0x0F0F0F0F', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], ['0', '0', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], ['0', '-1', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], ['0', '0', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['0', '0', '0', '-1'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.gt_u  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.gt_u  (i32x4) (i8x16)'])
        case_data.append(['gt_u', ['0xFFFFFFFF', '0xFF'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_u', ['4294967295', '255'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_u', ['0', '0'], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_u', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_u', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '0', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_u', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['-1', '0', '0', '0'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['gt_u', ['0x55555555', '0xAA'], '0', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.gt_u  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.gt_u  (i32x4) (i16x8)'])
        case_data.append(['gt_u', ['0xFFFFFFFF', '0xFFFF'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_u', ['4294967295', '65535'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_u', ['0', '0'], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_u', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '0', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_u', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], ['0', '0', '0', '0'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_u', [['-128', '0', '1', '255'], ['-128', '0', '1', '255']], ['-1', '0', '0', '0'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['gt_u', ['0xAAAAAAAA', '0x5555'], '-1', ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['gt_u', ['0_123_456_789', '123456789'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['gt_u', ['0x0_1234_5678', '0x12345678'], '0', ['i32x4', 'i32x4', 'i32x4']])

        # ge_s
        # i32x4.ge_s  (i32x4) (i32x4)
        case_data.append(['#', 'ge_s'])

        case_data.append(['#', 'i32x4.ge_s  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['ge_s', ['0xFFFFFFFF', '0xFFFFFFFF'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['0x00000000', '0x00000000'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['0xF0F0F0F0', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['0x0F0F0F0F', '0x0F0F0F0F'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['ge_s', ['0xFFFFFFFF', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['0xFFFFFFFF', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['0x80808080', '2155905152'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['0x80808080', '-2139062144'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['ge_s', ['-1', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['0', '0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['4294967295', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['4294967295', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['4294967295', '0'], ['4294967295', '0']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['0', '4294967295'], ['0', '4294967295']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['-2147483647', '4294967295', '0', '-1'], ['2147483649', '-1', '0', '-1']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['ge_s', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'], ['-128.0', '-127.0', '-1.0', '0.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['ge_s', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'], ['1.0', '127.0', '128.0', '255.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['ge_s', ['0x0F0F0F0F', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], ['-1', '-1', '0', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], ['-1', '-1', '-1', '0'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], ['0', '0', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['-1', '-1', '-1', '0'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.ge_s  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.ge_s  (i32x4) (i8x16)'])
        case_data.append(['ge_s', ['0xFFFFFFFF', '0xFF'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_s', ['4294967295', '255'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_s', ['0', '0'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_s', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_s', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_s', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']], ['-1', '-1', '0', '-1'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_s', ['0x55555555', '0x55'], '-1', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.ge_s  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.ge_s  (i32x4) (i16x8)'])
        case_data.append(['ge_s', ['0xFFFFFFFF', '0xFFFF'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_s', ['4294967295', '65535'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_s', ['0', '0'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_s', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_s', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_s', [['65535', '0', '1', '32768'], ['65535', '65535', '0', '0', '1', '1', '32768', '32768']], ['-1', '-1', '0', '-1'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_s', ['0xAAAAAAAA', '0x5555'], '0', ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['ge_s', ['0_123_456_789', '123456789'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_s', ['0x0_1234_5678', '0x12345678'], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # ge_u
        # i32x4.ge_u  (i32x4) (i32x4)
        case_data.append(['#', 'ge_u'])

        case_data.append(['#', 'i32x4.ge_u  (i32x4) (i32x4)'])

        # hex vs hex
        case_data.append(['#', 'hex vs hex'])
        case_data.append(['ge_u', ['0xFFFFFFFF', '0xFFFFFFFF'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['0x00000000', '0x00000000'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['0xF0F0F0F0', '0xF0F0F0F0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['0x0F0F0F0F', '0x0F0F0F0F'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['0xFFFFFFFF', '0x00000000'], ['0xFFFFFFFF', '0x00000000']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['0x00000000', '0xFFFFFFFF'], ['0x00000000', '0xFFFFFFFF']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B'],
                          ['0x03020100', '0x11100904', '0x1A0B0A12', '0xFFABAA1B']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs dec
        case_data.append(['#', 'hex vs dec'])
        case_data.append(['ge_u', ['0xFFFFFFFF', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['0xFFFFFFFF', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['0x80808080', '2155905152'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['0x80808080', '-2139062144'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['0x83828180', '0x00FFFEFD', '0x7F020100', '0xFFFEFD80'],
                          ['2206368128', '16776957', '2130837760', '4294901120']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # dec vs dec
        case_data.append(['#', 'dec vs dec'])
        case_data.append(['ge_u', ['-1', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['0', '0'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['4294967295', '4294967295'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['4294967295', '-1'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['4294967295', '0'], ['4294967295', '0']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['0', '4294967295'], ['0', '4294967295']], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['-2147483647', '4294967295', '0', '-1'], ['2147483649', '-1', '0', '-1']], '-1', ['i32x4', 'i32x4', 'i32x4']])

        # hex vs float
        case_data.append(['#', 'hex vs float'])
        case_data.append(['ge_u', [['0xc3000000', '0xc2fe0000', '0xbf800000', '0x00000000'], ['-128.0', '-127.0', '-1.0', '0.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])
        case_data.append(['ge_u', [['0x3f800000', '0x42fe0000', '0x43000000', '0x437f0000'], ['1.0', '127.0', '128.0', '255.0']], '-1', ['i32x4', 'f32x4', 'i32x4']])

        # not equal
        case_data.append(['#', 'not equal'])
        case_data.append(['ge_u', ['0x0F0F0F0F', '0xF0F0F0F0'], '0', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['0x00000000', '0xFFFFFFFF'], ['0xFFFFFFFF', '0x00000000']], ['0', '0', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['0x02030001', '0x10110409', '0x0B1A120A', '0xABFF1BAA'],
                          ['0xAA1BFFAB', '0x0A121A0B', '0x09041110', '0x01000302']], ['0', '-1', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['0x80018000', '0x80038002', '0x80058004', '0x80078006'],
                          ['2147975174', '2147844100', '2147713026', '2147581952']], ['0', '0', '-1', '-1'], ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', [['2147483648', '2147483647', '0', '-1'], ['-2147483648', '-2147483647', '-1', '0']], ['-1', '0', '0', '-1'], ['i32x4', 'i32x4', 'i32x4']])

        # i32x4.ge_u  (i32x4) (i8x16)
        case_data.append(['#', 'i32x4.ge_u  (i32x4) (i8x16)'])
        case_data.append(['ge_u', ['0xFFFFFFFF', '0xFF'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_u', ['4294967295', '255'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_u', ['0', '0'], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_u', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x00', '0x01', '0x02', '0x03', '0x04', '0x05', '0x06', '0x07', '0x08', '0x09', '0x0A', '0x0B', '0x0C', '0x0D', '0x0E', '0x0F']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_u', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['-128', '-127', '-126', '-125', '-3', '-2', '-1', '0', '0', '1', '2', '127', '128', '253', '254', '255']], '-1', ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_u', [['-8323200', '0', '1', '4294967295'], ['-128', '0', '1', '255']],
                          ['-1', '-1', '0', '-1'], ['i32x4', 'i8x16', 'i32x4']])
        case_data.append(['ge_u', ['0xAAAAAAAA', '0x55'], '-1', ['i32x4', 'i8x16', 'i32x4']])

        # i32x4.ge_u  (i32x4) (i16x8)
        case_data.append(['#', 'i32x4.ge_u  (i32x4) (i16x8)'])
        case_data.append(['ge_u', ['0xFFFFFFFF', '0xFFFF'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_u', ['4294967295', '65535'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_u', ['0', '0'], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_u', [['0x03020100', '0x07060504', '0x0B0A0908', '0x0F0E0D0C'],
                          ['0x0100', '0x0302', '0x0504', '0x0706', '0x0908', '0x0B0A', '0x0D0C', '0x0F0E']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_u', [['2206368128', '16776957', '2130837760', '4294901120'],
                          ['33152', '33666', '65277', '255', '256', '32514', '64896', '65534']], '-1', ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_u', [['-128', '0', '1', '255'], ['65535', '65535', '0', '0', '1', '1', '32768', '32768']], ['0', '-1', '0', '0'], ['i32x4', 'i16x8', 'i32x4']])
        case_data.append(['ge_u', ['0xAAAAAAAA', '0x5555'], ['-1', '-1', '-1', '-1'], ['i32x4', 'i16x8', 'i32x4']])

        case_data.append(['ge_u', ['0_123_456_789', '123456789'], '-1', ['i32x4', 'i32x4', 'i32x4']])
        case_data.append(['ge_u', ['0x0_1234_5678', '0x12345678'], '-1', ['i32x4', 'i32x4', 'i32x4']])

        return case_data

    # generate all test cases
    def get_all_cases(self):

        # Add tests for unkonow operators for i32x4
        return SimdCmpCase.get_all_cases(self) + """
;; Unknown operators

(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.eq (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.ne (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.lt_s (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.lt_u (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.le_s (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.le_u (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.gt_s (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.gt_u (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.ge_s (local.get $x) (local.get $y)))") "unknown operator")
(assert_malformed (module quote "(memory 1) (func (param $x v128) (param $y v128) (result v128) (i4x32.ge_u (local.get $x) (local.get $y)))") "unknown operator")

"""


def gen_test_cases():
    i32x4 = Simdi32x4CmpCase()
    i32x4.gen_test_cases()


if __name__ == '__main__':
    i32x4 = Simdi32x4CmpCase()
    i32x4.gen_test_cases()
