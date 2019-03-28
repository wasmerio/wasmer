use crate::emitter_x64::*;
use std::collections::HashSet;
use wasmparser::Type as WpType;

pub struct Machine<E: Emitter> {
    used_gprs: HashSet<GPR>,
    used_xmms: HashSet<XMM>,
    control: Vec<ControlFrame<E>>,
    stack_offset: usize,
}

#[derive(Debug)]
pub struct ControlFrame<E: Emitter> {
    pub label: E::Label,
    pub loop_like: bool,
    pub returns: Vec<WpType>,
    pub stack_offset_snapshot: usize,
}

impl<E: Emitter> Machine<E> {
    /// Picks an unused general purpose register.
    /// 
    /// This method does not mark the register as used.
    pub fn pick_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &'static [GPR] = &[
            //RAX,
            RCX,
            RDX,
            RBX,
            RSP,
            RBP,
            RSI,
            RDI,
            R8,
            R9,
            R10,
            R11,
            //R12,
            //R13,
            //R14,
            //R15
        ];
        for r in REGS {
            if !self.used_gprs.contains(r) {
                return Some(*r)
            }
        }
        None
    }

    /// Picks an unused XMM register.
    /// 
    /// This method does not mark the register as used.
    pub fn pick_xmm(&self) -> Option<XMM> {
        use XMM::*;
        static REGS: &'static [XMM] = &[
            //XMM0,
            //XMM1,
            //XMM2,
            XMM3,
            XMM4,
            XMM5,
            XMM6,
            XMM7,
        ];
        for r in REGS {
            if !self.used_xmms.contains(r) {
                return Some(*r)
            }
        }
        None
    }

    /// Acquires locations from the machine state.
    /// 
    /// If the returned locations are used for stack value, `release_location` needs to be called on them;
    /// Otherwise, if the returned locations are used for locals, `release_location` does not need to be called on them.
    pub fn acquire_locations(
        &mut self,
        assembler: &mut E,
        tys: &[WpType],
    ) -> Vec<Location> {
        let mut ret = vec![];
        let mut delta_stack_offset: usize = 0;

        for ty in tys {
            let loc = match *ty {
                WpType::F32 | WpType::F64 => {
                    self.pick_xmm().map(Location::XMM).or_else(
                        || self.pick_gpr().map(Location::GPR)
                    )
                },
                WpType::I32 | WpType::I64 => {
                    self.pick_gpr().map(Location::GPR)
                },
                _ => unreachable!()
            };
            
            let loc = if let Some(x) = loc {
                x
            } else {
                self.stack_offset += 8;
                delta_stack_offset += 8;
                Location::Memory(GPR::RBP, -(self.stack_offset as i32))
            };
            if let Location::GPR(x) = loc {
                self.used_gprs.insert(x);
            } else if let Location::XMM(x) = loc {
                self.used_xmms.insert(x);
            }
            ret.push(loc);
        }

        assembler.emit_sub(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP));
        ret
    }

    /// Releases locations used for stack value.
    pub fn release_locations(
        &mut self,
        assembler: &mut E,
        locs: &[Location]
    ) {
        let mut delta_stack_offset: usize = 0;

        for loc in locs {
            match *loc {
                Location::GPR(ref x) => {
                    assert_eq!(self.used_gprs.remove(x), true);
                },
                Location::XMM(ref x) => {
                    assert_eq!(self.used_xmms.remove(x), true);
                },
                Location::Memory(GPR::RBP, x) => {
                    if x >= 0 {
                        unreachable!();
                    }
                    let offset = (-x) as usize;
                    if offset != self.stack_offset {
                        unreachable!();
                    }
                    self.stack_offset -= 8;
                    delta_stack_offset += 8;
                },
                _ => {}
            }
        }

        assembler.emit_add(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP));
    }
}
