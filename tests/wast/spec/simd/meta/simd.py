#!/usr/bin/env python3

"""
This python file is a tool class for SIMD and
currently only supports generating v128 const constant data.
"""


class SIMD:

    # Constant template
    CONST = '({value_type}.const {value})'

    # v128 Constant template
    V128_CONST = '(v128.const {lane_type} {value})'

    @staticmethod
    def const(value, value_type):
        """
        generation constant data, [e.g. i32, i64, f32, f64]
        Params:
            value: constant data, string or list,
            lane_type: lane type, [i32, i64, f32, f64]
        """
        return SIMD.CONST.format(value_type=value_type, value=''.join(str(value)))

    @staticmethod
    def v128_const(value, lane_type):
        """
        generation v128 constant data, [e.g. i8x16, i16x8, i32x4, f32x4]
        Params:
            value: constant data, string or list,
            lane_type: lane type, [e.g. i8x16, i16x8, i32x4, f32x4]
        """
        if lane_type.lower().find('x') == -1:
            return SIMD.const(value, lane_type)

        lane_cnt = int(lane_type[1:].split('x')[1])

        # value is a string type, generating constant data
        # of value according to the number of lanes
        if isinstance(value, str):
            data_elem = [value] * lane_cnt

        # If value is type of list, generate constant data
        # according to combination of list contents and number of lanes
        elif isinstance(value, list):

            # If it is an empty list, generate all constant data with 0x00
            if len(value) == 0:
                return SIMD.v128_const('0x00', lane_type)

            data_elem = []

            # Calculate the number of times each element in value is copied
            times = lane_cnt // len(value)

            # Calculate whether the data needs to be filled according to
            # the number of elements in the value list and the number of lanes.
            complement = lane_cnt % len(value)
            complement_item = ''

            # If the number of elements in the value list is greater than the number of lanes,
            # paste data with the number of lanes from the value list.
            if times == 0:
                times = 1
                complement = 0

                value = value[0:lane_cnt]

            # Copy data
            for item in value:
                data_elem.extend([item] * times)
                complement_item = item

            # Fill in the data
            if complement > 0:
                data_elem.extend([complement_item] * complement)

        # Get string
        data_elem = ' '.join(data_elem)

        # Returns v128 constant text
        return SIMD.V128_CONST.format(lane_type=lane_type, value=data_elem)
