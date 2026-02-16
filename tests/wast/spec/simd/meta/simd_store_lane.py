#!/usr/bin/env python3

from simd import SIMD
from test_assert import AssertReturn, AssertInvalid

def list_stringify(l):
    return list(map(lambda x: str(x), l))

"""Base class for generating SIMD store lane tests. Subclasses only to:
    - define self.LANE_LEN, self.LANE_TYPE, self.NUM_LANES, self.MAX_ALIGN
    - override get_normal_case to provide test data  (consult comments for details)

It generates test cases that:
    - store to all valid lane indices
    - store using memarg offset
    - store with memarg alignment
    - store with invalid lane index
    - store with invalid memarg alignment
    - fails typecheck
"""
class SimdStoreLane:
    def valid_alignments(self):
        return [a for a in range(1, self.MAX_ALIGN+1) if a & (a-1) == 0]

    def get_case_data(self):
        # return value should be a list of tuples:
        #   (address to store to : i32, v128, return value : v128)
        # e.g. [(0, [0x0100, 0, 0, 0, 0, 0, 0, 0]), ... ]
        # the expected result is return_value[address].
        raise Exception("Subclasses should override this to provide test data")

    def get_normal_case(self):
        s = SIMD()
        cases = []

        # store using arg
        for (addr, ret) in self.get_case_data():
            i32_addr = s.const(addr, "i32")
            v128_val = s.v128_const(list_stringify(ret), self.LANE_TYPE)
            result = s.const(ret[addr], "i64")
            instr = "v128.store{lane_len}_lane_{idx}".format(lane_len=self.LANE_LEN, idx=addr)
            cases.append(str(AssertReturn(instr, [i32_addr, v128_val], result)))

        # store using offset
        for (addr, ret) in self.get_case_data():
            v128_val = s.v128_const(list_stringify(ret), self.LANE_TYPE)
            result = s.const(ret[addr], "i64")
            instr = "v128.store{lane_len}_lane_{idx}_offset_{idx}".format(lane_len=self.LANE_LEN, idx=addr)
            cases.append(str(AssertReturn(instr, [v128_val], result)))

        # store using offset with alignment
        for (addr, ret) in self.get_case_data():
            for align in self.valid_alignments():
                i32_addr = s.const(addr, "i32")
                v128_val = s.v128_const(list_stringify(ret), self.LANE_TYPE)
                result = s.const(ret[addr], "i64")
                instr = "v128.store{lane_len}_lane_{idx}_align_{align}".format(lane_len=self.LANE_LEN, idx=addr, align=align)
                cases.append(str(AssertReturn(instr, [i32_addr, v128_val], result)))

        return '\n'.join(cases)

    def gen_test_func_template(self):
        template = [
            ';; Tests for store lane operations.\n\n',
            '(module',
            '  (memory 1)',
            '  (global $zero (mut v128) (v128.const i32x4 0 0 0 0))',
            ]

        lane_indices = list(range(self.NUM_LANES))

        # store using i32.const arg
        for idx in lane_indices:
            template.append(
                '  (func (export "v128.store{lane_len}_lane_{idx}")\n'
                '    (param $address i32) (param $x v128) (result i64) (local $ret i64)\n'
                '    (v128.store{lane_len}_lane {idx} (local.get $address) (local.get $x))\n'
                '    (local.set $ret (i64.load (local.get $address)))\n'
                '    (v128.store (local.get $address) (global.get $zero))'
                '    (local.get $ret))'
                .format(idx=idx, lane_len=self.LANE_LEN))

        # store using memarg offset
        for idx in lane_indices:
            template.append(
                '  (func (export "v128.store{lane_len}_lane_{idx}_offset_{idx}")\n'
                '    (param $x v128) (result i64) (local $ret i64)\n'
                '    (v128.store{lane_len}_lane offset={idx} {idx} (i32.const 0) (local.get $x))\n'
                '    (local.set $ret (i64.load offset={idx} (i32.const 0)))\n'
                '    (v128.store offset={idx} (i32.const 0) (global.get $zero))\n'
                '    (local.get $ret))'
                .format(idx=idx, lane_len=self.LANE_LEN))

        # with memarg aligment
        for idx in lane_indices:
            for align in self.valid_alignments():
                template.append(
                    '  (func (export "v128.store{lane_len}_lane_{idx}_align_{align}")\n'
                    '    (param $address i32) (param $x v128) (result i64) (local $ret i64)\n'
                    '    (v128.store{lane_len}_lane align={align} {idx} (local.get $address) (local.get $x))\n'
                    '    (local.set $ret (i64.load (local.get $address)))\n'
                    '    (v128.store offset={idx} (i32.const 0) (global.get $zero))\n'
                    '    (local.get $ret))'
                    .format(idx=idx, lane_len=self.LANE_LEN, align=align))

        template.append(')\n')
        return template

    def gen_test_template(self):
        template = self.gen_test_func_template()

        template.append('{normal_cases}')
        template.append('\n{invalid_cases}')

        return '\n'.join(template)

    def get_invalid_cases(self):
        invalid_cases = [';; type check']
        invalid_cases.append(
            '(assert_invalid'
            '  (module (memory 1)\n'
            '          (func (param $x v128) (result v128)\n'
            '            (v128.store{lane_len}_lane 0 (local.get $x) (i32.const 0))))\n'
            '  "type mismatch")'.format(lane_len=self.LANE_LEN))
        invalid_cases.append('')

        invalid_cases.append(';; invalid lane index')
        invalid_cases.append(
            '(assert_invalid'
            '  (module (memory 1)\n'
            '          (func (param $x v128) (result v128)\n'
            '            (v128.store{lane_len}_lane {idx} (i32.const 0) (local.get $x))))\n'
            '  "invalid lane index")'.format(idx=self.NUM_LANES, lane_len=self.LANE_LEN))

        invalid_cases.append('')

        invalid_cases.append(';; invalid memarg alignment')
        invalid_cases.append(
            '(assert_invalid\n'
            '  (module (memory 1)\n'
            '          (func (param $x v128) (result v128)\n'
            '          (v128.store{lane_len}_lane align={align} 0 (i32.const 0) (local.get $x))))\n'
            '  "alignment must not be larger than natural")'
            .format(lane_len=self.LANE_LEN, align=self.MAX_ALIGN*2))
        return '\n'.join(invalid_cases)

    def get_all_cases(self):
        case_data = {'lane_len': self.LANE_LEN,
                     'normal_cases': self.get_normal_case(),
                     'invalid_cases': self.get_invalid_cases(),
                     }
        return self.gen_test_template().format(**case_data)

    def gen_test_cases(self):
        wast_filename = '../simd_store{lane_type}_lane.wast'.format(lane_type=self.LANE_LEN)
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())

class SimdStore8Lane(SimdStoreLane):
    LANE_LEN = '8'
    LANE_TYPE = 'i8x16'
    NUM_LANES = 16
    MAX_ALIGN = 1

    def get_case_data(self):
        return [
            (0, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (1, [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (2, [0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (3, [0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (4, [0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (5, [0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (6, [0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (7, [0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0]),
            (8, [0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0]),
            (9, [0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0]),
            (10, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0]),
            (11, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0]),
            (12, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0]),
            (13, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 0, 0]),
            (14, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 14, 0]),
            (15, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15])]

class SimdStore16Lane(SimdStoreLane):
    LANE_LEN = '16'
    LANE_TYPE = 'i16x8'
    NUM_LANES = 8
    MAX_ALIGN = 2

    def get_case_data(self):
        return [
            (0, [0x0100, 0, 0, 0, 0, 0, 0, 0]),
            (1, [0, 0x0201, 0, 0, 0, 0, 0, 0]),
            (2, [0, 0, 0x0302, 0, 0, 0, 0, 0]),
            (3, [0, 0, 0, 0x0403, 0, 0, 0, 0]),
            (4, [0, 0, 0, 0, 0x0504, 0, 0, 0]),
            (5, [0, 0, 0, 0, 0, 0x0605, 0, 0]),
            (6, [0, 0, 0, 0, 0, 0, 0x0706, 0]),
            (7, [0, 0, 0, 0, 0, 0, 0, 0x0807])]

class SimdStore32Lane(SimdStoreLane):
    LANE_LEN = '32'
    LANE_TYPE = 'i32x4'
    NUM_LANES = 4
    MAX_ALIGN = 4

    def get_case_data(self):
        return [
            (0, [0x03020100, 0, 0, 0,]),
            (1, [0, 0x04030201, 0, 0,]),
            (2, [0, 0, 0x05040302, 0,]),
            (3, [0, 0, 0, 0x06050403,])]

class SimdStore64Lane(SimdStoreLane):
    LANE_LEN = '64'
    LANE_TYPE = 'i64x2'
    NUM_LANES = 2
    MAX_ALIGN = 8

    def get_case_data(self):
        return [
            (0, [0x0706050403020100, 0]),
            (1, [0, 0x0807060504030201])]

def gen_test_cases():
    simd_store8_lane = SimdStore8Lane()
    simd_store8_lane.gen_test_cases()
    simd_store16_lane = SimdStore16Lane()
    simd_store16_lane.gen_test_cases()
    simd_store32_lane = SimdStore32Lane()
    simd_store32_lane.gen_test_cases()
    simd_store64_lane = SimdStore64Lane()
    simd_store64_lane.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
