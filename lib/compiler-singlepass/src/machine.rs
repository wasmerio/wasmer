use crate::common_decl::*;
use crate::emitter_x64::{Label, Offset};
use crate::location::{CombinedRegister, Location, Reg};
use smallvec::smallvec;
use smallvec::SmallVec;
use std::cmp;
use std::collections::HashSet;
use std::marker::PhantomData;
use wasmer_compiler::wasmparser::Type as WpType;
use wasmer_compiler::CallingConvention;

#[allow(dead_code)]
#[derive(Clone, PartialEq)]
pub enum Value {
    I8(i8),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

pub trait MaybeImmediate {
    fn imm_value(&self) -> Option<Value>;
    fn is_imm(&self) -> bool {
        self.imm_value().is_some()
    }
}

// all machine seems to have a page this size, so not per arch for now
const NATIVE_PAGE_SIZE: usize = 4096;

pub struct MachineStackOffset(usize);

pub trait MachineSpecific<R: Reg, S: Reg> {
    /// New MachineSpecific object
    fn new() -> Self;
    /// Get the GPR that hold vmctx
    fn get_vmctx_reg() -> R;
    /// Picks an unused general purpose register for local/stack/argument use.
    ///
    /// This method does not mark the register as used, but needs the used vector
    fn pick_gpr(&self, used_gpr: &HashSet<R>) -> Option<R>;
    /// Picks an unused general purpose register for internal temporary use.
    ///
    /// This method does not mark the register as used, but needs the used vector
    fn pick_temp_gpr(&self, used_gpr: &HashSet<R>) -> Option<R>;
    /// Picks an unused SIMD register.
    ///
    /// This method does not mark the register as used, but needs the used vector
    fn pick_simd(&self, used_simd: &HashSet<S>) -> Option<S>;
    /// Picks an unused SIMD register for internal temporary use.
    ///
    /// This method does not mark the register as used, but needs the used vector
    fn pick_temp_simd(&self, used_simd: &HashSet<S>) -> Option<S>;
    /// Memory location for a local on the stack
    /// Like Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32)) for x86_64
    fn local_on_stack(&mut self, stack_offset: i32) -> Location<R, S>;
    /// Adjust stack for locals
    /// Like assembler.emit_sub(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP))
    fn adjust_stack(&mut self, delta_stack_offset: u32);
    /// Pop stack of locals
    /// Like assembler.emit_add(Size::S64, Location::Imm32(delta_stack_offset as u32), Location::GPR(GPR::RSP))
    fn pop_stack_locals(&mut self, delta_stack_offset: u32);
    /// Zero a location taht is 32bits
    fn zero_location(&mut self, size: Size, location: Location<R, S>);
    /// GPR Reg used for local pointer on the stack
    fn local_pointer(&self) -> R;
    /// Determine whether a local should be allocated on the stack.
    fn is_local_on_stack(&self, idx: usize) -> bool;
    /// Determine a local's location.
    fn get_local_location(&self, idx: usize, callee_saved_regs_size: usize) -> Location<R, S>;
    /// Move a local to the stack
    /// Like emit_mov(Size::S64, location, Location::Memory(GPR::RBP, -(self.stack_offset.0 as i32)));
    fn move_local(&mut self, stack_offset: i32, location: Location<R, S>);
    /// List of register to save, depending on the CallingConvention
    fn list_to_save(&self, calling_convention: CallingConvention) -> Vec<Location<R, S>>;
    /// Get param location
    fn get_param_location(idx: usize, calling_convention: CallingConvention) -> Location<R, S>;
    /// move a location to another
    fn move_location(&mut self, size: Size, source: Location<R, S>, dest: Location<R, S>);
    /// Init the stack loc counter
    fn init_stack_loc(&mut self, init_stack_loc_cnt: u64, last_stack_loc: Location<R, S>);
    /// Restore save_area
    fn restore_saved_area(&mut self, saved_area_offset: i32);
    /// Pop a location
    fn pop_location(&mut self, location: Location<R, S>);
    /// Create a new `MachineState` with default values.
    fn new_machine_state() -> MachineState;

    /// Finalize the assembler
    fn assembler_finalize(self) -> Vec<u8>;

    /// get_offset of Assembler
    fn get_offset(&self) -> Offset;

    /// finalize a function
    fn finalize_function(&mut self);

    /// emit an Illegal Opcode
    fn emit_illegal_op(&mut self);
    /// create a new label
    fn get_label(&mut self) -> Label;
    /// emit a label
    fn emit_label(&mut self, label: Label);

    /// get the gpr use for call. like RAX on x86_64
    fn get_grp_for_call(&self) -> R;
    /// Emit a call using the value in register
    fn emit_call_register(&mut self, register: R);
    /// get the gpr for the return of generic values
    fn get_gpr_for_ret(&self) -> R;
    /// load the address of a memory location (will panic if src is not a memory)
    /// like LEA opcode on x86_64
    fn location_address(&mut self, size: Size, source: Location<R, S>, dest: Location<R, S>);

    /// And src & dst -> dst (with or without flags)
    fn location_and(
        &mut self,
        size: Size,
        source: Location<R, S>,
        dest: Location<R, S>,
        flags: bool,
    );

    /// Add src+dst -> dst (with or without flags)
    fn location_add(
        &mut self,
        size: Size,
        source: Location<R, S>,
        dest: Location<R, S>,
        flags: bool,
    );
    /// Cmp src - dst and set flags
    fn location_cmp(&mut self, size: Size, source: Location<R, S>, dest: Location<R, S>);
    /// Test src & dst and set flags
    fn location_test(&mut self, size: Size, source: Location<R, S>, dest: Location<R, S>);

    /// jmp on equal (src==dst)
    /// like Equal set on x86_64
    fn jmp_on_equal(&mut self, label: Label);
    /// jmp on different (src!=dst)
    /// like NotEqual set on x86_64
    fn jmp_on_different(&mut self, label: Label);
    /// jmp on above (src>dst)
    /// like Above set on x86_64
    fn jmp_on_above(&mut self, label: Label);
    /// jmp on overflow
    /// like Carry set on x86_64
    fn jmp_on_overflow(&mut self, label: Label);
}

pub struct Machine<R: Reg, S: Reg, M: MachineSpecific<R, S>, C: CombinedRegister> {
    used_gprs: HashSet<R>,
    used_simd: HashSet<S>,
    stack_offset: MachineStackOffset,
    save_area_offset: Option<MachineStackOffset>,
    pub state: MachineState,
    pub(crate) track_state: bool,
    pub specific: M,
    phantom: PhantomData<C>,
}

impl<R: Reg, S: Reg, M: MachineSpecific<R, S>, C: CombinedRegister> Machine<R, S, M, C> {
    pub fn new() -> Self {
        Machine {
            used_gprs: HashSet::new(),
            used_simd: HashSet::new(),
            stack_offset: MachineStackOffset(0),
            save_area_offset: None,
            state: M::new_machine_state(),
            track_state: true,
            specific: M::new(),
            phantom: PhantomData,
        }
    }

    pub fn get_stack_offset(&self) -> usize {
        self.stack_offset.0
    }

    pub fn get_used_gprs(&self) -> Vec<R> {
        self.used_gprs.iter().cloned().collect()
    }

    pub fn get_used_simd(&self) -> Vec<S> {
        self.used_simd.iter().cloned().collect()
    }

    pub fn get_vmctx_reg() -> R {
        M::get_vmctx_reg()
    }

    /// Acquires a temporary GPR.
    pub fn acquire_temp_gpr(&mut self) -> Option<R> {
        let gpr = self.specific.pick_temp_gpr(&self.used_gprs);
        if let Some(x) = gpr {
            self.used_gprs.insert(x);
        }
        gpr
    }

    /// Releases a temporary GPR.
    pub fn release_temp_gpr(&mut self, gpr: R) {
        assert!(self.used_gprs.remove(&gpr));
    }

    /// Specify that a given register is in use.
    pub fn reserve_unused_temp_gpr(&mut self, gpr: R) -> R {
        assert!(!self.used_gprs.contains(&gpr));
        self.used_gprs.insert(gpr);
        gpr
    }

    /// Acquires a temporary XMM register.
    pub fn acquire_temp_simd(&mut self) -> Option<S> {
        let simd = self.specific.pick_temp_simd(&self.used_simd);
        if let Some(x) = simd {
            self.used_simd.insert(x);
        }
        simd
    }

    /// Releases a temporary XMM register.
    pub fn release_temp_simd(&mut self, simd: S) {
        assert_eq!(self.used_simd.remove(&simd), true);
    }

    /// Get param location
    pub fn get_param_location(idx: usize, calling_convention: CallingConvention) -> Location<R, S> {
        M::get_param_location(idx, calling_convention)
    }

    /// Acquires locations from the machine state.
    ///
    /// If the returned locations are used for stack value, `release_location` needs to be called on them;
    /// Otherwise, if the returned locations are used for locals, `release_location` does not need to be called on them.
    pub fn acquire_locations(
        &mut self,
        tys: &[(WpType, MachineValue)],
        zeroed: bool,
    ) -> SmallVec<[Location<R, S>; 1]> {
        let mut ret = smallvec![];
        let mut delta_stack_offset: usize = 0;

        for (ty, mv) in tys {
            let loc = match *ty {
                WpType::F32 | WpType::F64 => {
                    self.specific.pick_simd(&self.used_simd).map(Location::SIMD)
                }
                WpType::I32 | WpType::I64 => {
                    self.specific.pick_gpr(&self.used_gprs).map(Location::GPR)
                }
                WpType::FuncRef | WpType::ExternRef => {
                    self.specific.pick_gpr(&self.used_gprs).map(Location::GPR)
                }
                _ => unreachable!("can't acquire location for type {:?}", ty),
            };

            let loc = if let Some(x) = loc {
                x
            } else {
                self.stack_offset.0 += 8;
                delta_stack_offset += 8;
                self.specific.local_on_stack(self.stack_offset.0 as i32)
            };
            if let Location::GPR(x) = loc {
                self.used_gprs.insert(x);
                self.state.register_values[C::from_gpr(x.into_index() as u16).to_index().0] =
                    mv.clone();
            } else if let Location::SIMD(x) = loc {
                self.used_simd.insert(x);
                self.state.register_values[C::from_simd(x.into_index() as u16).to_index().0] =
                    mv.clone();
            } else {
                self.state.stack_values.push(mv.clone());
            }
            self.state.wasm_stack.push(WasmAbstractValue::Runtime);
            ret.push(loc);
        }

        if delta_stack_offset != 0 {
            self.specific.adjust_stack(delta_stack_offset as u32);
        }
        if zeroed {
            for i in 0..tys.len() {
                self.specific.zero_location(Size::S64, ret[i]);
            }
        }
        ret
    }

    /// Releases locations used for stack value.
    pub fn release_locations(&mut self, locs: &[Location<R, S>]) {
        let mut delta_stack_offset: usize = 0;

        for loc in locs.iter().rev() {
            match *loc {
                Location::GPR(ref x) => {
                    assert_eq!(self.used_gprs.remove(x), true);
                    self.state.register_values[C::from_gpr(x.into_index() as u16).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::SIMD(ref x) => {
                    assert_eq!(self.used_simd.remove(x), true);
                    self.state.register_values[C::from_simd(x.into_index() as u16).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::Memory(y, x) => {
                    if y == self.specific.local_pointer() {
                        if x >= 0 {
                            unreachable!();
                        }
                        let offset = (-x) as usize;
                        if offset != self.stack_offset.0 {
                            unreachable!();
                        }
                        self.stack_offset.0 -= 8;
                        delta_stack_offset += 8;
                        self.state.stack_values.pop().unwrap();
                    }
                }
                _ => {}
            }
            self.state.wasm_stack.pop().unwrap();
        }

        if delta_stack_offset != 0 {
            self.specific.adjust_stack(delta_stack_offset as u32);
        }
    }

    pub fn release_locations_only_regs(&mut self, locs: &[Location<R, S>]) {
        for loc in locs.iter().rev() {
            match *loc {
                Location::GPR(ref x) => {
                    assert_eq!(self.used_gprs.remove(x), true);
                    self.state.register_values[C::from_gpr(x.into_index() as u16).to_index().0] =
                        MachineValue::Undefined;
                }
                Location::SIMD(ref x) => {
                    assert_eq!(self.used_simd.remove(x), true);
                    self.state.register_values[C::from_simd(x.into_index() as u16).to_index().0] =
                        MachineValue::Undefined;
                }
                _ => {}
            }
            // Wasm state popping is deferred to `release_locations_only_osr_state`.
        }
    }

    pub fn release_locations_only_stack(&mut self, locs: &[Location<R, S>]) {
        let mut delta_stack_offset: usize = 0;

        for loc in locs.iter().rev() {
            if let Location::Memory(y, x) = *loc {
                if y == self.specific.local_pointer() {
                    if x >= 0 {
                        unreachable!();
                    }
                    let offset = (-x) as usize;
                    if offset != self.stack_offset.0 {
                        unreachable!();
                    }
                    self.stack_offset.0 -= 8;
                    delta_stack_offset += 8;
                    self.state.stack_values.pop().unwrap();
                }
            }
            // Wasm state popping is deferred to `release_locations_only_osr_state`.
        }

        if delta_stack_offset != 0 {
            self.specific.pop_stack_locals(delta_stack_offset as u32);
        }
    }

    pub fn release_locations_only_osr_state(&mut self, n: usize) {
        let new_length = self
            .state
            .wasm_stack
            .len()
            .checked_sub(n)
            .expect("release_locations_only_osr_state: length underflow");
        self.state.wasm_stack.truncate(new_length);
    }

    pub fn release_locations_keep_state(&mut self, locs: &[Location<R, S>]) {
        let mut delta_stack_offset: usize = 0;
        let mut stack_offset = self.stack_offset.0;

        for loc in locs.iter().rev() {
            if let Location::Memory(y, x) = *loc {
                if y == self.specific.local_pointer() {
                    if x >= 0 {
                        unreachable!();
                    }
                    let offset = (-x) as usize;
                    if offset != stack_offset {
                        unreachable!();
                    }
                    stack_offset -= 8;
                    delta_stack_offset += 8;
                }
            }
        }

        if delta_stack_offset != 0 {
            self.specific.pop_stack_locals(delta_stack_offset as u32);
        }
    }

    pub fn init_locals(
        &mut self,
        n: usize,
        n_params: usize,
        calling_convention: CallingConvention,
    ) -> Vec<Location<R, S>> {
        // How many machine stack slots will all the locals use?
        let num_mem_slots = (0..n)
            .filter(|&x| self.specific.is_local_on_stack(x))
            .count();

        // Total size (in bytes) of the pre-allocated "static area" for this function's
        // locals and callee-saved registers.
        let mut static_area_size: usize = 0;

        // Callee-saved registers used for locals.
        // Keep this consistent with the "Save callee-saved registers" code below.
        for i in 0..n {
            // If a local is not stored on stack, then it is allocated to a callee-saved register.
            if !self.specific.is_local_on_stack(i) {
                static_area_size += 8;
            }
        }

        // Callee-saved R15 for vmctx.
        static_area_size += 8;

        // Some ABI (like Windows) needs extrat reg save
        static_area_size += 8 * self.specific.list_to_save(calling_convention).len();

        // Total size of callee saved registers.
        let callee_saved_regs_size = static_area_size;

        // Now we can determine concrete locations for locals.
        let locations: Vec<Location<R, S>> = (0..n)
            .map(|i| self.specific.get_local_location(i, callee_saved_regs_size))
            .collect();

        // Add size of locals on stack.
        static_area_size += num_mem_slots * 8;

        // Allocate save area, without actually writing to it.
        self.specific.adjust_stack(static_area_size as _);

        // Save callee-saved registers.
        for loc in locations.iter() {
            if let Location::GPR(x) = *loc {
                self.stack_offset.0 += 8;
                self.specific.move_local(self.stack_offset.0 as i32, *loc);
                self.state.stack_values.push(MachineValue::PreserveRegister(
                    C::from_gpr(x.into_index() as u16).to_index(),
                ));
            }
        }

        // Save R15 for vmctx use.
        self.stack_offset.0 += 8;
        self.specific.move_local(
            self.stack_offset.0 as i32,
            Location::GPR(M::get_vmctx_reg()),
        );
        self.state.stack_values.push(MachineValue::PreserveRegister(
            C::from_gpr(M::get_vmctx_reg().into_index() as u16).to_index(),
        ));

        // Check if need to same some CallingConvention specific regs
        let regs_to_save = self.specific.list_to_save(calling_convention);
        for loc in regs_to_save.iter() {
            self.stack_offset.0 += 8;
            self.specific.move_local(self.stack_offset.0 as i32, *loc);
        }

        // Save the offset of register save area.
        self.save_area_offset = Some(MachineStackOffset(self.stack_offset.0));

        // Save location information for locals.
        for (i, loc) in locations.iter().enumerate() {
            match *loc {
                Location::GPR(x) => {
                    self.state.register_values[C::from_gpr(x.into_index() as u16).to_index().0] =
                        MachineValue::WasmLocal(i);
                }
                Location::Memory(_, _) => {
                    self.state.stack_values.push(MachineValue::WasmLocal(i));
                }
                _ => unreachable!(),
            }
        }

        // Load in-register parameters into the allocated locations.
        // Locals are allocated on the stack from higher address to lower address,
        // so we won't skip the stack guard page here.
        for i in 0..n_params {
            let loc = Self::get_param_location(i + 1, calling_convention);
            self.specific.move_location(Size::S64, loc, locations[i]);
        }

        // Load vmctx into R15.
        self.specific.move_location(
            Size::S64,
            Self::get_param_location(0, calling_convention),
            Location::GPR(M::get_vmctx_reg()),
        );

        // Stack probe.
        //
        // `rep stosq` writes data from low address to high address and may skip the stack guard page.
        // so here we probe it explicitly when needed.
        for i in (n_params..n).step_by(NATIVE_PAGE_SIZE / 8).skip(1) {
            self.specific.zero_location(Size::S64, locations[i]);
        }

        // Initialize all normal locals to zero.
        let mut init_stack_loc_cnt = 0;
        let mut last_stack_loc = Location::Memory(self.specific.local_pointer(), i32::MAX);
        for i in n_params..n {
            match locations[i] {
                Location::Memory(_, _) => {
                    init_stack_loc_cnt += 1;
                    last_stack_loc = cmp::min(last_stack_loc, locations[i]);
                }
                Location::GPR(_) => {
                    self.specific.zero_location(Size::S64, locations[i]);
                }
                _ => unreachable!(),
            }
        }
        if init_stack_loc_cnt > 0 {
            self.specific
                .init_stack_loc(init_stack_loc_cnt, last_stack_loc);
        }

        // Add the size of all locals allocated to stack.
        self.stack_offset.0 += static_area_size - callee_saved_regs_size;

        locations
    }

    pub fn finalize_locals(
        &mut self,
        locations: &[Location<R, S>],
        calling_convention: CallingConvention,
    ) {
        // Unwind stack to the "save area".
        self.specific
            .restore_saved_area(self.save_area_offset.as_ref().unwrap().0 as i32);

        let regs_to_save = self.specific.list_to_save(calling_convention);
        for loc in regs_to_save.iter().rev() {
            self.specific.pop_location(*loc);
        }

        // Restore register used by vmctx.
        self.specific
            .pop_location(Location::GPR(M::get_vmctx_reg()));

        // Restore callee-saved registers.
        for loc in locations.iter().rev() {
            if let Location::GPR(_) = *loc {
                self.specific.pop_location(*loc);
            }
        }
    }

    pub fn assembler_get_offset(&self) -> Offset {
        self.specific.get_offset()
    }

    pub fn assembler_finalize(self) -> Vec<u8> {
        self.specific.assembler_finalize()
    }
}
