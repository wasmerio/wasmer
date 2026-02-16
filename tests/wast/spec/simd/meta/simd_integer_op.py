#!/usr/bin/env python3

"""Common integer value operations"""

from simd_lane_value import LaneValue


class ArithmeticOp:
    """This class provides methods to simulate integer arithmetic
    and saturating integer arithmetic operations for the purpose of
    getting corresponding expected results. One or more operands
    may be required for the operations.
    The following operators are covered:
    add, sub, mul, neg,
    add_sat_s, add_sat_u,
    sub_sat_s, sub_sat_u,
    min_s, min_u, max_s, max_u, avgr_u, abs
    ext_mul_s, ext_mul_u
    """
    def __init__(self, op: str):
        self.op = op

    @staticmethod
    def get_valid_value(value: int, lane: LaneValue, signed=True) -> int:
        """Get the valid integer value in the scope of the specified lane size.

        For a integer value, convert it to the valid value with the same bits
        of the lane width. The value can be signed or unsigned, with the scope
        of -0x80... to 0x7f... or 0 to 0xff...

        :param value: the value of the integer
        :param lane: the LaneValue instance of a lane in v128
        :param signed: specify if the lane is interpreted as a signed or
                        an unsigned number.
        :return : the valid value in either signed or unsigned number
        """
        value &= lane.mask
        if signed:
            if value > lane.max:
                return value - lane.mod
            if value < lane.min:
                return value + lane.mod
        return value

    def _saturate(self, operand1: int, operand2: int, lane: LaneValue) -> int:
        """Get the result of saturating arithmetic operation on 2 operands.
        The operands can be both signed or unsigned. The following ops
        are covered:
        add_sat_s, sub_sat_s, add_sat_u, sub_sat_u,

        Saturating arithmetic can make sure:
        When the operation result is less than the minimum, return the minimum.
        When the operation result is greater than the maximum, return the maximum.
        For other operation results, simply return themselves.
        :param operand1: the integer operand 1
        :param operand2: the integer operand 2
        :param lane: the LaneValue instance of a lane in v128
        :return: the result of the saturating arithmetic operation
        """
        if self.op.endswith('sat_s'):
            if operand1 > lane.max:
                operand1 -= lane.mod
            if operand2 > lane.max:
                operand2 -= lane.mod

            if self.op.startswith('add'):
                value = operand1 + operand2
            if self.op.startswith('sub'):
                value = operand1 - operand2

            return lane.sat_s(value)

        if self.op.endswith('sat_u'):
            if operand1 < 0:
                operand1 += lane.mod
            if operand2 < 0:
                operand2 += lane.mod
            if self.op.startswith('add'):
                value = operand1 + operand2
            if self.op.startswith('sub'):
                value = operand1 - operand2

            return lane.sat_u(value)

        return value

    def unary_op(self, operand, lane):
        """General integer arithmetic and saturating arithmetic operations
        with only one operand.

        Supported ops: neg, abs

        :param operand: the operand, integer or literal string in hex or decimal format
        :param lane: the LaneValue instance of a lane in v128
        :return: the string of the result of <self.op p> in hex or decimal format
        """
        v = operand
        base = 10
        if isinstance(operand, str):
            if '0x' in operand:
                base = 16
            v = int(operand, base)

        if self.op == 'neg':
            result = self.get_valid_value(-v, lane)
        elif self.op == 'abs':
            result = self.get_valid_value(v, lane)
            if result >= 0:
                return operand
            else:
                result = -result
            if base == 16:
                return hex(result)
        elif self.op == 'popcnt':
            result = self.get_valid_value(v, lane)
            return str(bin(result % lane.mod).count('1'))
        elif self.op == 'sat_s':
            # Don't call get_valid_value, it will truncate results.
            return lane.sat_s(v)
        elif self.op == 'sat_u':
            # Don't call get_valid_value, it will truncate results.
            return lane.sat_u(v)
        else:
            raise Exception('Unknown unary operation')

        return str(result)

    def binary_op(self, operand1, operand2, src_lane, dst_lane=None):
        """General integer arithmetic and saturating arithmetic operations
        with 2 operands.

        Supported ops:
        add, sub, mul,
        add_sat_s, add_sat_u,
        sub_sat_s, sub_sat_u,
        min_s, min_u, max_s, max_u, avgr_u,
        ext_mul_s, ext_mul_u (same as mul),
        q15mulr_sat_s

        :param operand1: the operand 1, integer or literal string in hex or decimal format
        :param operand2: the operand 2, integer or literal string in hex or decimal format
        :param src_lane: the LaneValue instance of a lane in v128
        :return: the string of the result of <p1 self.op p2> in hex or decimal format
        """
        if not dst_lane:
            dst_lane = src_lane
        v1 = operand1
        v2 = operand2
        base1 = base2 = 10
        if isinstance(operand1, str):
            if '0x' in operand1:
                base1 = 16
            v1 = int(operand1, base1)
        if isinstance(operand2, str):
            if '0x' in operand2:
                base2 = 16
            v2 = int(operand2, base2)

        result_signed = True
        if self.op == 'add':
            value = v1 + v2
        elif self.op == 'sub':
            value = v1 - v2
        elif self.op == 'mul':
            value = v1 * v2
        elif self.op.startswith('extmul_'):
            if self.op.endswith('s'):
                i1 = self.get_valid_value(v1, src_lane)
                i2 = self.get_valid_value(v2, src_lane)
            else:
                i1 = self.get_valid_value(v1, src_lane, signed=False)
                i2 = self.get_valid_value(v2, src_lane, signed=False)
            value = i1 * i2
        elif self.op == 'q15mulr_sat_s':
            # This should be before 'sat' case.
            i1 = ArithmeticOp.get_valid_value(v1, src_lane)
            i2 = ArithmeticOp.get_valid_value(v2, src_lane)
            return src_lane.sat_s((i1 * i2 + 0x4000) >> 15)
        elif 'sat' in self.op:
            value = self._saturate(v1, v2, src_lane)
            if self.op.endswith('_u'):
                result_signed = False
        elif self.op in ['min_s', 'max_s']:
            i1 = self.get_valid_value(v1, src_lane)
            i2 = self.get_valid_value(v2, src_lane)
            if self.op == 'min_s':
                return operand1 if i1 <= i2 else operand2
            else:
                return operand1 if i1 >= i2 else operand2
        elif self.op in ['min_u', 'max_u']:
            i1 = self.get_valid_value(v1, src_lane, signed=False)
            i2 = self.get_valid_value(v2, src_lane, signed=False)
            if self.op == 'min_u':
                return operand1 if i1 <= i2 else operand2
            else:
                return operand1 if i1 >= i2 else operand2
        elif self.op == 'avgr_u':
            i1 = self.get_valid_value(v1, src_lane, signed=False)
            i2 = self.get_valid_value(v2, src_lane, signed=False)
            result = (i1 + i2 + 1) // 2
            if base1 == 16 or base2 == 16:
                return hex(result)
            else:
                return str(result)
        else:
            raise Exception('Unknown binary operation')

        result = self.get_valid_value(value, dst_lane, signed=result_signed)
        return str(result)
