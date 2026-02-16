#!/usr/bin/env python3


class LaneValue:
    """This class stands for the value of signed integer represented by a lane in v128.
    Suppose a bit number of the lane is n, then:
    For signed integer:
        minimum = -pow(2, n - 1), maximum = pow(2, n - 1) - 1
    The bit number of the lane can be 8, 16, 32, 64"""
    def __init__(self, lane_width):
        """lane_width: bit number of each lane in SIMD v128"""
        self.lane_width = lane_width

    @property
    def min(self):
        return -pow(2, self.lane_width - 1)

    @property
    def max(self):
        return pow(2, self.lane_width - 1) - 1

    @property
    def mask(self):
        return pow(2, self.lane_width) - 1

    @property
    def mod(self):
        return pow(2, self.lane_width)

    @property
    def quarter(self):
        return pow(2, self.lane_width - 2)

    def sat_s(self, v):
        return max(self.min, min(v, self.max))

    def sat_u(self, v):
        return max(0, min(v, self.mask))
