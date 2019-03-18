use crate::utils::regs::Reg;
use nom::{
    count, do_parse, le_i32, le_u16, le_u32, le_u64, le_u8, map, named, tuple, value, verify,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Header {
    pub version: u8,
    pub reserved0: u8,
    pub reserved1: u16,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct StackSizeRecord {
    pub function_addr: u64,
    pub stack_size: u64,
    pub record_count: u64,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum LocationValue {
    /// Value is in a register,
    Register { reg: Reg },

    /// Frame index value.
    Direct { reg: Reg, offset: i32 },

    /// Spilled value.
    Indirect { reg: Reg, offset: i32 },

    /// Small constant.
    Constant { value: u32 },

    /// Large constant.
    ConstIndex { index: u32 },
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Location {
    pub value: LocationValue,
    pub size: u16,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct LiveOut {
    pub reg: Reg,
    pub size_in_bytes: u8,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StackMapRecord {
    pub patchpoint_id: u64,
    pub inst_offset: u32,
    pub locations: Box<[Location]>,
    pub live_outs: Box<[LiveOut]>,
}

pub struct Stackmap {
    pub header: Header,
    pub stack_size_records: Box<[StackSizeRecord]>,
    pub constants: Box<[u64]>,
    pub stack_map_records: Box<[StackMapRecord]>,
}

impl Stackmap {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        parse_stackmap(bytes)
            .map(|(_, stackmap)| stackmap)
            .map_err(|e| format!("{}", e))
    }
}

named!(parse_header<&[u8], Header>, do_parse!(
    version: verify!(le_u8, |version: u8| version == 3) >>
    reserved0: verify!(le_u8, |reserved: u8| reserved == 0) >>
    reserved1: verify!(le_u16, |reserved: u16| reserved == 0) >>

    (Header {
        version,
        reserved0,
        reserved1,
    })
));

named!(parse_stack_size_record<&[u8], StackSizeRecord>, do_parse!(
    function_addr: le_u64 >>
    stack_size: le_u64 >>
    record_count: le_u64 >>

    (StackSizeRecord {
        function_addr,
        stack_size,
        record_count,
    })
));

named!(parse_location_value<&[u8], LocationValue>, do_parse!(
    loc: verify!(le_u8, |loc: u8| match loc {
        0 | 1 | 2 | 3 | 4 => true,
        _ => false,
    }) >>
        le_u8 >>
    reg: map!(le_u16, Reg::from) >>
        le_u16 >>
    offset_or_const: le_i32 >>
    (match loc {
        // Register.
        0 => LocationValue::Register { reg },
        // Direct.
        1 => LocationValue::Direct { reg, offset: offset_or_const },
        // Indirect.
        2 => LocationValue::Indirect { reg, offset: offset_or_const },
        // Constant.
        3 => LocationValue::Constant { value: offset_or_const as u32 },
        // ConstIndex.
        4 => LocationValue::ConstIndex { index: offset_or_const as u32 },
        _ => unreachable!(),
    })
));

named!(parse_location<&[u8], Location>, do_parse!(
    value: parse_location_value >>
    size: le_u16 >>

    (Location {
        value,
        size,
    })
));

named!(parse_live_out<&[u8], LiveOut>, do_parse!(
    reg: map!(le_u16, Reg::from) >>
        le_u8 >>
    size_in_bytes: le_u8 >>

    (LiveOut {
        reg,
        size_in_bytes,
    })
));

named!(parse_stack_map_record<&[u8], StackMapRecord>, do_parse!(
    patchpoint_id: le_u64 >>
    inst_offset: le_u32 >>
        le_u16 >>
    num_locations: map!(le_u16, u16_as_usize) >>
    locations: count!(parse_location, num_locations) >>
        le_u32 >>
        le_u16 >>
    num_live_outs: map!(le_u16, u16_as_usize) >>
    live_outs: count!(parse_live_out, num_live_outs) >>
    (StackMapRecord {
        patchpoint_id,
        inst_offset,
        locations: locations.into_boxed_slice(),
        live_outs: live_outs.into_boxed_slice(),
    })
));

named!(parse_stackmap<&[u8], Stackmap>, do_parse!(
    header: parse_header >>

    num_functions: map!(le_u32, u32_as_usize) >>
    num_constants: map!(le_u32, u32_as_usize) >>
    num_records: map!(le_u32, u32_as_usize) >>

    stack_size_records: count!(parse_stack_size_record, num_functions) >>
    constants: count!(le_u64, num_constants) >>
    stack_map_records: count!(parse_stack_map_record, num_records) >>

    (Stackmap {
        header,
        stack_size_records: stack_size_records.into_boxed_slice(),
        constants: constants.into_boxed_slice(),
        stack_map_records: stack_map_records.into_boxed_slice(),
    })
));

fn u32_as_usize(i: u32) -> usize {
    i as usize
}

fn u16_as_usize(i: u16) -> usize {
    i as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header() {
        let header = vec![0x3, 0, 0, 0];

        assert_eq!(
            parse_header(&header),
            Ok((
                &[] as &[u8],
                Header {
                    version: 3,
                    reserved0: 0,
                    reserved1: 0,
                }
            ))
        );
    }

    #[test]
    fn test_parse_stack_size_record() {
        let record = vec![
            0xbe, 0xba, 0xfe, 0xca, 0xbe, 0xba, 0xfe, 0xca, 0xbe, 0xba, 0xfe, 0xca, 0xbe, 0xba,
            0xfe, 0xca, 0xbe, 0xba, 0xfe, 0xca, 0xbe, 0xba, 0xfe, 0xca,
        ];

        assert_eq!(
            parse_stack_size_record(&record),
            Ok((
                &[] as &[u8],
                StackSizeRecord {
                    function_addr: 0xcafebabecafebabe,
                    stack_size: 0xcafebabecafebabe,
                    record_count: 0xcafebabecafebabe,
                }
            ))
        );
    }
}
