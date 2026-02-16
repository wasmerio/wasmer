#!/usr/bin/env python3

from simd_arithmetic import SimdArithmeticCase, i16
from simd_integer_op import ArithmeticOp


class SimdExtAddPairwise(SimdArithmeticCase):
    BINARY_OPS = ()

    def unary_op(self, x, signed):
        # For test data we always splat a single value to the
        # entire v128, so doubling the input works.
        return ArithmeticOp.get_valid_value(x, self.src_lane, signed=signed) * 2

    @property
    def hex_unary_op_test_data(self):
        return []

    @property
    def unary_test_data(self):
        return [
            (self.normal_unary_op_test_data, [self.SRC_LANE_TYPE,self.LANE_TYPE]),
        ]

    def get_case_data(self):
        case_data = []
        for op in self.UNARY_OPS:
            op_name = self.op_name(op)
            case_data.append(['#', op_name])
            for data_group, v128_forms in self.unary_test_data:
                for data in data_group:
                    case_data.append([op_name, [str(data)],
                        str(self.unary_op(data, op.endswith('s'))),
                        v128_forms])
        return case_data

    def get_combine_cases(self):
        return ''

    def gen_test_cases(self):
        wast_filename = '../simd_{}_extadd_pairwise_{}.wast'.format(self.LANE_TYPE, self.SRC_LANE_TYPE)
        with open(wast_filename, 'w') as fp:
            fp.write(self.get_all_cases())

class SimdI16x8ExtAddPairwise(SimdExtAddPairwise):
    UNARY_OPS = ('extadd_pairwise_i8x16_s','extadd_pairwise_i8x16_u')
    LANE_TYPE = 'i16x8'
    SRC_LANE_TYPE = 'i8x16'

class SimdI32x4ExtAddPairwise(SimdExtAddPairwise):
    UNARY_OPS = ('extadd_pairwise_i16x8_s','extadd_pairwise_i16x8_u')
    LANE_TYPE = 'i32x4'
    SRC_LANE_TYPE = 'i16x8'

def gen_test_cases():
    simd_i16x8_arith = SimdI16x8ExtAddPairwise()
    simd_i32x4_arith = SimdI32x4ExtAddPairwise()
    simd_i16x8_arith.gen_test_cases()
    simd_i32x4_arith.gen_test_cases()

if __name__ == '__main__':
    gen_test_cases()
