use crate::emitter_x64::*;
use std::collections::HashSet;
use wasmparser::Type as WpType;

struct MachineStackOffset(usize);

pub struct Machine {
    used_gprs: HashSet<GPR>,
    used_xmms: HashSet<XMM>,
    stack_offset: MachineStackOffset
}

impl Machine {
    pub fn new() -> Self {
        Machine {
            used_gprs: HashSet::new(),
            used_xmms: HashSet::new(),
            stack_offset: MachineStackOffset(0),
        }
    }

    /// Picks an unused general purpose register for local/stack/argument use.
    /// 
    /// This method does not mark the register as used.
    pub fn pick_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &'static [GPR] = &[
            RCX,
            RDX,
            RBX,
            RSI,
            RDI,
            R8,
            R9,
            R10,
            R11,
        ];
        for r in REGS {
            if !self.used_gprs.contains(r) {
                return Some(*r)
            }
        }
        None
    }

    /// Picks an unused general purpose register for internal temporary use.
    /// 
    /// This method does not mark the register as used.
    pub fn pick_temp_gpr(&self) -> Option<GPR> {
        use GPR::*;
        static REGS: &'static [GPR] = &[
            RAX,
            R12,
            R13,
            R14,
            R15
        ];
        for r in REGS {
            if !self.used_gprs.contains(r) {
                return Some(*r)
            }
        }
        None
    }

    /// Acquires a temporary GPR.
    pub fn acquire_temp_gpr(&mut self) -> Option<GPR> {
        let gpr = self.pick_temp_gpr();
        if let Some(x) = gpr {
            self.used_gprs.insert(x);
        }
        gpr
    }

    /// Releases a temporary GPR.
    pub fn release_temp_gpr(&mut self, gpr: GPR) {
        assert_eq!(self.used_gprs.remove(&gpr), true);
    }

    /// Picks an unused XMM register.
    /// 
    /// This method does not mark the register as used.
    pub fn pick_xmm(&self) -> Option<XMM> {
        use XMM::*;
        static REGS: &'static [XMM] = &[
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

    /// Picks an unused XMM register for internal temporary use.
    /// 
    /// This method does not mark the register as used.
    pub fn pick_temp_xmm(&self) -> Option<XMM> {
        use XMM::*;
        static REGS: &'static [XMM] = &[
            XMM0,
            XMM1,
            XMM2,
        ];
        for r in REGS {
            if !self.used_xmms.contains(r) {
                return Some(*r)
            }
        }
        None
    }

    /// Acquires a temporary XMM register.
    pub fn acquire_temp_xmm(&mut self) -> Option<XMM> {
        let xmm = self.pick_temp_xmm();
        if let Some(x) = xmm {
            self.used_xmms.insert(x);
        }
        xmm
    }

    /// Releases a temporary XMM register.
    pub fn release_temp_xmm(&mut self, xmm: XMM) {
        assert_eq!(self.used_xmms.remove(&xmm), true);
    }

    /// Acquires locations from the machine state.
    /// 
    /// If the returned locations are used for stack value, `release_location` needs to be called on them;
    /// Otherwise, if the returned locations are used for locals, `release_location` does not need to be called on them.
    pub fn acquire_locations<E: Emitter>(
        &mut self,
        assembler: &mut E,
        tys: &[WpType],
        zeroed: bool,
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
                self.stack_offset.0 += 8;
                delta_stack_offset += 8;
                Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32))
            };
            if let Location::GPR(x) = loc {
                self.used_gprs.insert(x);
            } else if let Location::XMM(x) = loc {
                self.used_xmms.insert(x);
            }
            ret.push(loc);
        }

        assembler.emit_sub(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP));
        if zeroed {
            for i in 0..tys.len() {
                assembler.emit_mov(Size::S64, Location::Imm32(0), Location::Memory(GPR::RSP, (i * 8) as i32));
            }
        }
        ret
    }

    /// Releases locations used for stack value.
    pub fn release_locations<E: Emitter>(
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
                    if offset != self.stack_offset.0 {
                        unreachable!();
                    }
                    self.stack_offset.0 -= 8;
                    delta_stack_offset += 8;
                },
                _ => {}
            }
        }

        assembler.emit_add(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP));
    }
}
