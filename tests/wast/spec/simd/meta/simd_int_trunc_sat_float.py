#!/usr/bin/env python3

"""Base class for generating SIMD <int>.trun_sat_<float> test cases.
Subclasses should set:
    - LANE_TYPE
    - SRC_LANE_TYPE
    - UNARY_OPS
"""

from abc import abstractmethod
import struct
from math import trunc
from simd import SIMD
from simd_arithmetic import SimdArithmeticCase
from test_assert import AssertReturn
from simd_float_op import FloatingPointOp, FloatingPointRoundingOp
from simd_integer_op import ArithmeticOp


class SimdConversionCase(SimdArithmeticCase):
    BINARY_OPS = ()
    TEST_FUNC_TEMPLATE_HEADER = ";; Tests for {} trunc sat conversions from float.\n"

    def is_signed(self, op):
        return op.endswith("_s") or op.endswith("_s_zero")

    def get_test_data(self, lane):
        return [
            "0.0",
            "-0.0",
            "1.5",
            "-1.5",
            "1.9",
            "2.0",
            "-1.9",
            "-2.0",
            str(float(lane.max - 127)),
            str(float(-(lane.max - 127))),
            str(float(lane.max + 1)),
            str(float(-(lane.max + 1))),
            str(float(lane.max * 2)),
            str(float(-(lane.max * 2))),
            str(float(lane.max)),
            str(float(-lane.max)),
            str(float(lane.mask - 1)),
            str(float(lane.mask)),
            str(float(lane.mask + 1)),
            "0x1p-149",
            "-0x1p-149",
            "0x1p-126",
            "-0x1p-126",
            "0x1p-1",
            "-0x1p-1",
            "0x1p+0",
            "-0x1p+0",
            "0x1.19999ap+0",
            "-0x1.19999ap+0",
            "0x1.921fb6p+2",
            "-0x1.921fb6p+2",
            "0x1.fffffep+127",
            "-0x1.fffffep+127",
            "0x1.ccccccp-1",
            "-0x1.ccccccp-1",
            "0x1.fffffep-1",
            "-0x1.fffffep-1",
            "0x1.921fb6p+2",
            "-0x1.921fb6p+2",
            "0x1.fffffep+127",
            "-0x1.fffffep+127",
            "+inf",
            "-inf",
            "+nan",
            "-nan",
            "nan:0x444444",
            "-nan:0x444444",
            "42",
            "-42",
            "0123456792.0",
            "01234567890.0",
        ]

    def to_float_precision(self, value):
        # Python supports double precision, so given an an input that cannot be
        # precisely represented in f32, we need to round it.
        return value

    @abstractmethod
    def to_results(self, result: str):
        # Subclasses can override this to set the shape of the results. This is
        # useful if instructions zero top lanes.
        pass

    def conversion_op(self, op, operand):
        fop = FloatingPointRoundingOp()
        signed = self.is_signed(op)
        sat_op = ArithmeticOp("sat_s") if signed else ArithmeticOp("sat_u")
        result = fop.unary_op("trunc", operand, hex_form=False)
        if result == "nan":
            return "0"
        elif result == "+inf":
            return str(str(self.lane.max) if signed else str(self.lane.mask))
        elif result == "-inf":
            return str(self.lane.min if signed else 0)
        else:
            float_result = self.to_float_precision(float(result))
            trunced = int(trunc(float_result))
            saturated = sat_op.unary_op(trunced, self.lane)
            return str(saturated)

    def get_case_data(self):
        test_data = []
        for op in self.UNARY_OPS:
            op_name = "{}.{}".format(self.LANE_TYPE, op)
            test_data.append(["#", op_name])

            for operand in self.get_test_data(self.lane):
                operand = str(operand)
                if "nan" in operand:
                    test_data.append(
                        [op_name, [operand], "0", [self.SRC_LANE_TYPE, self.LANE_TYPE]]
                    )
                else:
                    result = self.conversion_op(op_name, operand)
                    results = self.to_results(result)
                    assert "nan" not in result
                    test_data.append(
                        [
                            op_name,
                            [operand],
                            results,
                            [self.SRC_LANE_TYPE, self.LANE_TYPE],
                        ]
                    )

        return test_data

    def gen_test_cases(self):
        wast_filename = "../simd_{}_trunc_sat_{}.wast".format(
            self.LANE_TYPE, self.SRC_LANE_TYPE
        )
        with open(wast_filename, "w") as fp:
            fp.write(self.get_all_cases())

    def get_combine_cases(self):
        return ""


class SimdI32x4TruncSatF32x4Case(SimdConversionCase):
    LANE_TYPE = "i32x4"
    SRC_LANE_TYPE = "f32x4"
    UNARY_OPS = ("trunc_sat_f32x4_s", "trunc_sat_f32x4_u")

    def to_float_precision(self, value):
        fop = FloatingPointOp()
        return fop.to_single_precision(value)

    def to_results(self, value: str):
        return [value]


class SimdI32x4TruncSatF64x2Case(SimdConversionCase):
    LANE_TYPE = "i32x4"
    SRC_LANE_TYPE = "f64x2"
    UNARY_OPS = ("trunc_sat_f64x2_s_zero", "trunc_sat_f64x2_u_zero")

    def to_results(self, value: str):
        return [value, "0"]


def gen_test_cases():
    i32x4_trunc_sat = SimdI32x4TruncSatF32x4Case()
    i32x4_trunc_sat.gen_test_cases()
    i32x4_trunc_sat = SimdI32x4TruncSatF64x2Case()
    i32x4_trunc_sat.gen_test_cases()


if __name__ == "__main__":
    gen_test_cases()
