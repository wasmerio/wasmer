#!/usr/bin/env python3

"""
Generates all integer-to-integer extension test cases.
"""

from simd import SIMD
from simd_arithmetic import SimdArithmeticCase
from test_assert import AssertReturn, AssertInvalid


class SimdIntToIntExtend(SimdArithmeticCase):
    LANE_TYPE = ""  # unused, can be anything
    BINARY_OPS = ()
    UNARY_OPS = (
        "i16x8.extend_high_i8x16_s",
        "i16x8.extend_high_i8x16_u",
        "i16x8.extend_low_i8x16_s",
        "i16x8.extend_low_i8x16_u",
        "i32x4.extend_high_i16x8_s",
        "i32x4.extend_high_i16x8_u",
        "i32x4.extend_low_i16x8_s",
        "i32x4.extend_low_i16x8_u",
        "i64x2.extend_high_i32x4_s",
        "i64x2.extend_high_i32x4_u",
        "i64x2.extend_low_i32x4_s",
        "i64x2.extend_low_i32x4_u",
    )

    TEST_FUNC_TEMPLATE_HEADER = ";; Tests for int-to-int extension operations.\n"

    def op_name(self, op):
        # Override base class implementation, since the lane type is already
        # part of the op name.
        return "{op}".format(lane_type=self.LANE_TYPE, op=op)

    def is_unsigned(self, op):
        return op.endswith("_u")

    def src_lane_type(self, op):
        return op[-7:-2]

    def dst_lane_type(self, op):
        return op[0:5]

    def get_test_cases(self, src_value):
        return [
            (0, 0),
            (0, 1),
            (0, -1),
            (1, 0),
            (-1, 0),
            (1, -1),
            ((-1, 1)),
            ((src_value.max - 1), (src_value.max)),
            ((src_value.max), (src_value.max - 1)),
            ((src_value.max), (src_value.max)),
            ((src_value.min), (src_value.min)),
            ((src_value.max), (src_value.min)),
            ((src_value.min), (src_value.max)),
            ((src_value.max), -1),
            (-1, (src_value.max)),
            (((src_value.min + 1), (src_value.min))),
            ((src_value.min), (src_value.min + 1)),
            ((src_value.min), (-1)),
            ((-1), (src_value.min)),
        ]

    def get_normal_case(self):
        cases = []

        for op in self.UNARY_OPS:
            src_lane_type = self.src_lane_type(op)
            src_value = self.LANE_VALUE[src_lane_type]
            operands = self.get_test_cases(src_value)

            for (low, high) in operands:
                result = low if "low" in op else high

                if self.is_unsigned(op):
                    # Unsign-extend, mask top bits.
                    result = result & src_value.mask

                cases.append(
                    str(
                        AssertReturn(
                            op,
                            [SIMD.v128_const([str(low), str(high)], src_lane_type)],
                            SIMD.v128_const(str(result), self.dst_lane_type(op)),
                        )
                    )
                )

            cases.append("")

        return "\n".join(cases)

    def gen_test_cases(self):
        wast_filename = "../simd_int_to_int_extend.wast"
        with open(wast_filename, "w") as fp:
            fp.write(self.get_all_cases())

    def get_combine_cases(self):
        return ""


def gen_test_cases():
    simd_int_to_int_extend = SimdIntToIntExtend()
    simd_int_to_int_extend.gen_test_cases()


if __name__ == "__main__":
    gen_test_cases()
