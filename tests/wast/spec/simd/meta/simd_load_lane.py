#!/usr/bin/env python3

from simd import SIMD
from test_assert import AssertReturn, AssertInvalid

def list_stringify(l):
    return list(map(lambda x: str(x), l))

"""Base class for generating SIMD load lane tests. Subclasses only to:
    - define self.LANE_LEN, self.LANE_TYPE, self.NUM_LANES, self.MAX_ALIGN
    - override get_normal_case to provide test data  (consult comments for details)

It generates test cases that:
    - load to all valid lane indices
    - load using memarg offset
    - load with memarg alignment
    - load with invalid lane index
    - load with invalid memarg alignment
    - fails typecheck
"""
class SimdLoadLane:
    def valid_alignments(self):
        return [a for a in range(1, self.MAX_ALIGN+1) if a & (a-1) == 0]

    def get_case_data(self):
        # return value should be a list of tuples:
        #   (address to load from : i32, initial value : v128, return value : v128)
        # e.g. [(0, [0], [0x0100, 0, 0, 0, 0, 0, 0, 0]), ... ]
        raise Exception("Subclasses should override this to provide test data")

    def get_normal_case(self):
        s = SIMD()
        cases = []

        # load using arg
        for (addr, val, ret) in self.get_case_data():
            i32_addr = s.const(addr, "i32")
            v128_val = s.v128_const(list_stringify(val), self.LANE_TYPE)
            v128_result = s.v128_const(list_stringify(ret), self.LANE_TYPE)
            instr = "v128.load{lane_len}_lane_{idx}".format(lane_len=self.LANE_LEN, idx=addr)
            cases.append(str(AssertReturn(instr, [i32_addr, v128_val], v128_result)))

        # load using offset
        for (addr, val, ret) in self.get_case_data():
            v128_val = s.v128_const(list_stringify(val), self.LANE_TYPE)
            v128_result = s.v128_const(list_stringify(ret), self.LANE_TYPE)
            instr = "v128.load{lane_len}_lane_{idx}_offset_{idx}".format(lane_len=self.LANE_LEN, idx=addr)
            cases.append(str(AssertReturn(instr, [v128_val], v128_result)))

        # load using offset with alignment
        for (addr, val, ret) in self.get_case_data():
            for align in self.valid_alignments():
                i32_addr = s.const(addr, "i32")
                v128_val = s.v128_const(list_stringify(val), self.LANE_TYPE)
                v128_result = s.v128_const(list_stringify(ret), self.LANE_TYPE)
                instr = "v128.load{lane_len}_lane_{idx}_align_{align}".format(lane_len=self.LANE_LEN, idx=addr, align=align)
                cases.append(str(AssertReturn(instr, [i32_addr, v128_val], v128_result)))

        return '\n'.join(cases)

    def gen_test_func_template(self):
        template = [
            ';; Tests for load lane operations.\n\n',
            '(module',
            '  (memory 1)',
            '  (data (i32.const 0) "\\00\\01\\02\\03\\04\\05\\06\\07\\08\\09\\0A\\0B\\0C\\0D\\0E\\0F")',
            ]

        lane_indices = list(range(self.NUM_LANES))

        # load using i32.const arg
        for idx in lane_indices:
            template.append(
                '  (func (export "v128.load{lane_len}_lane_{idx}")\n'
                '    (param $address i32) (param $x v128) (result v128)\n'
                '    (v128.load{lane_len}_lane {idx} (local.get $address) (local.get $x)))'
                .format(idx=idx, lane_len=self.LANE_LEN))

        # load using memarg offset
        for idx in lane_indices:
            template.append(
                '  (func (export "v128.load{lane_len}_lane_{idx}_offset_{idx}")\n'
                '    (param $x v128) (result v128)\n'
                '    (v128.load{lane_len}_lane offset={idx} {idx} (i32.const 0) (local.get $x)))'
                .format(idx=idx, lane_len=self.LANE_LEN))

        # with memarg aligment
        for idx in lane_indices:
            for align in self.valid_alignments():
                template.append(
                    '  (func (export "v128.load{lane_len}_lane_{idx}_align_{align}")\n'
                    '    (param $address i32) (param $x v128) (result v128)\n'
                    '    (v128.load{lane_len}_lane align={align} {idx} (local.get $address) (local.get $x)))'
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
            '            (v128.load{lane_len}_lane 0 (local.get $x) (i32.const 0))))\n'
            '  "type mismatch")'.format(lane_len=self.LANE_LEN))
        invalid_cases.append('')

        invalid_cases.append(';; invalid lane index')
        invalid_cases.append(
            '(assert_invalid'
            '  (module (memory 1)\n'
            '          (func (param $x v128) (result v128)\n'
            '            (v128.load{lane_len}_lane {idx} (i32.const 0) (local.get $x))))\n'
            '  "invalid lane index")'.format(idx=self.NUM_LANES, lane_len=self.LANE_LEN))

        invalid_cases.append('')

        invalid_cases.append(';; invalid memarg alignment')
        invalid_cases.append(
            '(assert_invalid\n'
            '  (module (memory 1)\n'
            '          (func (param $x v128) (result v128)\n'
            '          (v128.load{lane_len}_lane align={align} 0 (i32.const 0) (local.get $x))))\n'
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
        wast_filename = '../simd_load{lane_type}_lane.wast'.format(lane_type=self.LANE_LEN)
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())

class SimdLoad8Lane(SimdLoadLane):
    LANE_LEN = '8'
    LANE_TYPE = 'i8x16'
    NUM_LANES = 16
    MAX_ALIGN = 1

    def get_case_data(self):
        return [
            (0, [0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (1, [0], [0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (2, [0], [0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (3, [0], [0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (4, [0], [0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (5, [0], [0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (6, [0], [0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (7, [0], [0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0]),
            (8, [0], [0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0]),
            (9, [0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0]),
            (10, [0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0, 0]),
            (11, [0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 0]),
            (12, [0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0]),
            (13, [0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 0, 0]),
            (14, [0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 14, 0]),
            (15, [0], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 15])]

class SimdLoad16Lane(SimdLoadLane):
    LANE_LEN = '16'
    LANE_TYPE = 'i16x8'
    NUM_LANES = 8
    MAX_ALIGN = 2

    def get_case_data(self):
        return [
            (0, [0], [0x0100, 0, 0, 0, 0, 0, 0, 0]),
            (1, [0], [0, 0x0201, 0, 0, 0, 0, 0, 0]),
            (2, [0], [0, 0, 0x0302, 0, 0, 0, 0, 0]),
            (3, [0], [0, 0, 0, 0x0403, 0, 0, 0, 0]),
            (4, [0], [0, 0, 0, 0, 0x0504, 0, 0, 0]),
            (5, [0], [0, 0, 0, 0, 0, 0x0605, 0, 0]),
            (6, [0], [0, 0, 0, 0, 0, 0, 0x0706, 0]),
            (7, [0], [0, 0, 0, 0, 0, 0, 0, 0x0807])]

class SimdLoad32Lane(SimdLoadLane):
    LANE_LEN = '32'
    LANE_TYPE = 'i32x4'
    NUM_LANES = 4
    MAX_ALIGN = 4

    def get_case_data(self):
        return [
            (0, [0], [0x03020100, 0, 0, 0,]),
            (1, [0], [0, 0x04030201, 0, 0,]),
            (2, [0], [0, 0, 0x05040302, 0,]),
            (3, [0], [0, 0, 0, 0x06050403,])]

class SimdLoad64Lane(SimdLoadLane):
    LANE_LEN = '64'
    LANE_TYPE = 'i64x2'
    NUM_LANES = 2
    MAX_ALIGN = 8

    def get_case_data(self):
        return [
            (0, [0], [0x0706050403020100, 0]),
            (1, [0], [0, 0x0807060504030201])]

def gen_test_cases():
    simd_load8_lane = SimdLoad8Lane()
    simd_load8_lane.gen_test_cases()
    simd_load16_lane = SimdLoad16Lane()
    simd_load16_lane.gen_test_cases()
    simd_load32_lane = SimdLoad32Lane()
    simd_load32_lane.gen_test_cases()
    simd_load64_lane = SimdLoad64Lane()
    simd_load64_lane.gen_test_cases()


if __name__ == '__main__':
    gen_test_cases()
