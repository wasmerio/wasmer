use crate::machine::{MaybeImmediate};
use crate::codegen::{Local, WeakLocal};
use crate::common_decl::Size;
use wasmer::Value;
use wasmer_compiler::wasmparser::Type as WpType;
use std::marker::PhantomData;
use std::fmt::Debug;
use smallvec::{smallvec, SmallVec};

pub trait Emitter<R> {
    fn grow_stack(&mut self, count: usize) -> usize;
    fn shrink_stack(&mut self, offset: u32);
    fn move_imm32_to_reg(&mut self, sz: Size, val: u32, reg: R);
    fn move_imm32_to_mem(&mut self, sz: Size, val: u32, base: R, offset: i32);
    fn move_reg_to_reg(&mut self, sz: Size, reg1: R, reg2: R);
    fn move_reg_to_mem(&mut self, sz: Size, reg: R, base: R, offset: i32);
    fn move_mem_to_reg(&mut self, sz: Size, base: R, offset: i32, reg: R);
    fn move_mem_to_mem(&mut self, sz: Size, base1: R, offset1: i32, base2: R, offset2: i32);
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Location<R> {
    Reg(R),
    Memory(R, i32),
    Imm32(u32),
    None,
}

impl<R> MaybeImmediate for Location<R> {
    fn imm_value(&self) -> Option<Value> {
        match *self {
            Location::Imm32(imm) => Some(Value::I32(imm as i32)),
            _ => None
        }
    }
}

pub trait Reg: Copy + Clone + Eq + PartialEq + Debug {
    fn is_callee_save(self) -> bool;
    fn is_reserved(self) -> bool;
    fn into_index(self) -> usize;
    fn from_index(i: usize) -> Result<Self, ()>;
}

pub trait Descriptor<R: Reg> {
    const FP: R;
    const VMCTX: R;
    const REG_COUNT: usize;
    const WORD_SIZE: usize;
    const STACK_GROWS_DOWN: bool;
    const FP_STACK_ARG_OFFSET: i32;
    const ARG_REG_COUNT: usize;
    fn callee_save_regs() -> Vec<R>;
    fn caller_save_regs() -> Vec<R>;
    fn callee_param_location(n: usize) -> Location<R>;
    fn caller_arg_location(n: usize) -> Location<R>;
    fn return_location() -> Location<R>;
}

pub struct In2Out1<'a, R: Reg, E: Emitter<R>> {
    commutative: bool,
    size: Size,
    max_imm_width: u8,
    reg_imm_reg: Option<Box<dyn FnOnce(&mut E, R, u32, R) + 'a>>,
    reg_reg_reg: Option<Box<dyn FnOnce(&mut E, R, R, R) + 'a>>,
    reg_imm: Option<Box<dyn FnOnce(&mut E, R, u32) + 'a>>,
    reg_reg: Option<Box<dyn FnOnce(&mut E, R, R) + 'a>>,
    reg_exact_reg: Option<(Box<dyn FnOnce(&mut E, R) + 'a>, R, &'a[R])>,
}

impl<'a, R: Reg, E: Emitter<R>> In2Out1<'a, R, E> {
    pub fn new() -> Self {
        Self {
            commutative: false,
            size: Size::S32,
            max_imm_width: 255,
            reg_imm_reg: None,
            reg_reg_reg: None,
            reg_imm: None,
            reg_reg: None,
            reg_exact_reg: None,
        }
    }
    pub fn commutative(mut self, commutative: bool) -> Self {
        self.commutative = commutative;
        self
    }
    pub fn size(mut self, sz: Size) -> Self {
        self.size = sz;
        self
    }
    pub fn max_imm_width(mut self, max_imm_width: u8) -> Self {
        assert!(max_imm_width > 0);
        self.max_imm_width = max_imm_width;
        self
    }
    pub fn reg_imm_reg<F: FnOnce(&mut E, R, u32, R) + 'a>(mut self, reg_imm_reg: F) -> Self {
        self.reg_imm_reg = Some(Box::new(reg_imm_reg));
        self
    }
    pub fn reg_reg_reg<F: FnOnce(&mut E, R, R, R) + 'a>(mut self, reg_reg_reg: F) -> Self {
        self.reg_reg_reg = Some(Box::new(reg_reg_reg));
        self
    }
    pub fn reg_imm<F: FnOnce(&mut E, R, u32) + 'a>(mut self, reg_imm: F) -> Self {
        self.reg_imm = Some(Box::new(reg_imm));
        self
    }
    pub fn reg_reg<F: FnOnce(&mut E, R, R) + 'a>(mut self, reg_reg: F) -> Self {
        self.reg_reg = Some(Box::new(reg_reg));
        self
    }
    pub fn reg_exact_reg_with_clobber<F: FnOnce(&mut E, R) + 'a>(
        mut self, exact: R, clobbers: &'a[R], reg_exact_reg: F) -> Self {
        self.reg_exact_reg = Some((Box::new(reg_exact_reg), exact, clobbers));
        self
    }
    pub fn execute<D: Descriptor<R>>(
        self, manager: &mut LocalManager<R, E, D>, emitter: &'a mut E,
        src1: Local<Location<R>>, src2: Local<Location<R>>) -> Local<Location<R>> {
        manager.in2_out1(emitter, self, src1, src2)
    }
}

pub struct In2Out0<'a, R: Reg, E: Emitter<R>> {
    reg_reg: Option<Box<dyn FnOnce(&mut E, R, R) + 'a>>,
    _size: Size,
}

impl<'a, R: Reg, E: Emitter<R>> In2Out0<'a, R, E> {
    pub fn new() -> Self {
        Self {
            reg_reg: None,
            _size: Size::S32,
        }
    }
    pub fn size(mut self, sz: Size) -> Self {
        self._size = sz;
        self
    }
    pub fn reg_reg<F: FnOnce(&mut E, R, R) + 'a>(mut self, reg_reg: F) -> Self {
        self.reg_reg = Some(Box::new(reg_reg));
        self
    }
    pub fn execute<D: Descriptor<R>>(self, manager: &'a mut LocalManager<R, E, D>, emitter: &'a mut E,
        src1: Local<Location<R>>, src2: Local<Location<R>>) {
        manager.in2_out0(emitter, self, src1, src2);
    }
}

pub struct In1Out1<'a, R: Reg, E: Emitter<R>> {
    size: Size,
    reg_reg: Option<Box<dyn FnOnce(&mut E, R, R) + 'a>>,
}

impl<'a, R: Reg, E: Emitter<R>> In1Out1<'a, R, E> {
    pub fn new() -> Self {
        Self {
            size: Size::S32,
            reg_reg: None,
        }
    }
    pub fn size(mut self, sz: Size) -> Self {
        self.size = sz;
        self
    }
    pub fn reg_reg<F: FnOnce(&mut E, R, R) + 'a>(mut self, reg_reg: F) -> Self {
        self.reg_reg = Some(Box::new(reg_reg));
        self
    }
    pub fn execute<D: Descriptor<R>>(self, manager: &'a mut LocalManager<R, E, D>, emitter: &'a mut E,
        src: Local<Location<R>>) -> Local<Location<R>> {
        manager.in1_out1(emitter, self, src)
    }
}

pub struct In1Out0<'a, R: Reg, E: Emitter<R>> {
    reg: Option<Box<dyn FnOnce(&mut E, R) + 'a>>,
}

impl<'a, R: Reg, E: Emitter<R>> In1Out0<'a, R, E> {
    pub fn new() -> Self {
        Self {
            reg: None,
        }
    }
    pub fn reg<F: FnOnce(&mut E, R) + 'a>(mut self, reg: F) -> Self {
        self.reg = Some(Box::new(reg));
        self
    }
    pub fn execute<D: Descriptor<R>>(self, manager: &'a mut LocalManager<R, E, D>, emitter: &'a mut E, src: Local<Location<R>>) {
        manager.in1_out0(emitter, self, src);
    }
}

pub struct In0Out1<'a, R: Reg, E: Emitter<R>> {
    reg: Option<Box<dyn FnOnce(&mut E, R) + 'a>>,
    size: Size,
}

impl<'a, R: Reg, E: Emitter<R>> In0Out1<'a, R, E> {
    pub fn new() -> Self {
        Self {
            size: Size::S32,
            reg: None,
        }
    }
    pub fn size(mut self, sz: Size) -> Self {
        self.size = sz;
        self
    }
    pub fn reg<F: FnOnce(&mut E, R) + 'a>(mut self, reg: F) -> Self {
        self.reg = Some(Box::new(reg));
        self
    }
    pub fn execute<D: Descriptor<R>>(self, manager: &'a mut LocalManager<R, E, D>, emitter: &'a mut E) -> Local<Location<R>> {
        manager.in0_out1(emitter, self)
    }
}

pub struct LocalManager<R: Reg, E: Emitter<R>, D: Descriptor<R>> {
    regs: Vec<WeakLocal<Location<R>>>,
    stack: Vec<WeakLocal<Location<R>>>,
    reg_counter: usize,
    n_stack_params: usize,
    stack_offset: i32,
    free_regs: Vec<R>,
    free_callee_save: Vec<R>,
    free_stack: Vec<i32>,
    saved_free_regs: Vec<Vec<R>>,
    saved_free_callee_save: Vec<Vec<R>>,
    saved_free_stack: Vec<Vec<i32>>,
    saved_stack_offsets: Vec<i32>,
    _emitter: PhantomData<E>,
    _descriptor: PhantomData<D>,
}

impl<R: Reg, E: Emitter<R>, D: Descriptor<R>> LocalManager<R, E, D> {
    pub fn new() -> Self {
        Self {
            regs: (0..D::REG_COUNT).map(|_| WeakLocal::new()).collect(),
            stack: Vec::new(),
            reg_counter: 0,
            n_stack_params: 0,
            free_regs: vec![],
            free_callee_save: vec![],
            free_stack: vec![],
            stack_offset: 0,
            saved_stack_offsets: vec![],
            saved_free_regs: vec![],
            saved_free_callee_save: vec![],
            saved_free_stack: vec![],
            _emitter: PhantomData,
            _descriptor: PhantomData,
        }
    }
    
    pub fn get_stack_offset(&self) -> i32 {
        self.stack_offset
    }

    pub fn init_locals(&mut self, n_params: usize, n_locals: usize, free_regs: &[R]) -> Vec<Local<Location<R>>> {
        let param_locations: Vec<_> = (0..n_params).map(|i| D::callee_param_location(i + 1)).collect();
        let mut locals: Vec<Local<Location<R>>> = Vec::with_capacity(n_locals);
        
        // + 1 because the first arg is vmctx
        // this ASSUMES that reg args are passed first; if that's not the case, we'll need to add additional logic here
        self.n_stack_params = (n_params + 1).saturating_sub(D::ARG_REG_COUNT);
        for _ in 0..self.n_stack_params {
            self.stack.push(WeakLocal::new());
        }

        for loc in param_locations {
            let local = Local::new(loc, Size::S32);
            match loc {
                Location::Reg(reg) => {
                    self.regs[reg.into_index()] = local.downgrade();
                },
                Location::Memory(base, offset) => {
                    assert!(base == D::FP);
                    let idx = self.stack_idx(offset);
                    self.stack[idx] = local.downgrade();
                },
                _ => {
                    unreachable!();
                }
            }
            locals.push(local);
        }
        
        for _ in n_params..n_locals {
            locals.push(Local::new(Location::Imm32(0), Size::S32));
        }
        
        for &reg in free_regs {
            if reg.is_callee_save() {
                self.free_callee_save.push(reg);
            } else {
                self.free_regs.push(reg);
            }
        }

        locals
    }

    fn stack_idx(&self, offset: i32) -> usize {
        if (D::STACK_GROWS_DOWN && offset >= 0) || (!D::STACK_GROWS_DOWN && offset <= 0) {
            assert!(offset >= D::FP_STACK_ARG_OFFSET);
            (offset - D::FP_STACK_ARG_OFFSET) as usize / D::WORD_SIZE
        } else {
            self.n_stack_params + (offset / -(D::WORD_SIZE as i32)) as usize - 1
        }
    }

    // assumes the returned stack index will be used
    fn get_free_stack(&mut self, emitter: &mut E) -> i32 {
        if let Some(idx) = self.free_stack.pop() {
            return idx;
        }

        let inc = if D::STACK_GROWS_DOWN { -(D::WORD_SIZE as i32) } else { D::WORD_SIZE as i32 };

        let count = emitter.grow_stack(1);
        for _ in 0..count {
            self.stack_offset += inc;
            self.free_stack.push(self.stack_offset);
            self.stack.push(WeakLocal::new());
        }

        self.free_stack.pop().unwrap()
    }

    // loc.location() must be a register
    fn move_to_stack(&mut self, emitter: &mut E, loc: Local<Location<R>>) {
        let offset = self.get_free_stack(emitter);
        
        match loc.location() {
            Location::Imm32(imm) => {
                emitter.move_imm32_to_mem(loc.size(), imm, D::FP, offset);
            },
            Location::Reg(reg) => {
                emitter.move_reg_to_mem(loc.size(), reg, D::FP, offset);
            },
            Location::Memory(base, old_offset) => {
                assert!(base == D::FP);
                emitter.move_mem_to_mem(loc.size(), D::FP, old_offset, D::FP, offset);
            },
            _ => {
                unreachable!();
            }
        }

        self.release_location(loc.clone());
        loc.replace_location(Location::Memory(D::FP, offset));
        let idx = self.stack_idx(offset);
        self.stack[idx] = loc.downgrade();
    }

    pub fn restore_stack_offset(&mut self, emitter: &mut E, prev_offset: i32) -> bool {
        if self.stack_offset == prev_offset {
            return false;
        }

        let diff = (self.stack_offset - prev_offset).abs() as u32;
        emitter.shrink_stack(diff);
        true
    }

    pub fn block_begin(&mut self) {
        self.saved_stack_offsets.push(self.stack_offset);
        self.saved_free_regs.push(self.free_regs.clone());
        self.saved_free_callee_save.push(self.free_callee_save.clone());
        self.saved_free_stack.push(self.free_stack.clone());
    }

    pub fn block_end(&mut self, emitter: &mut E) {
        self.free_regs = self.saved_free_regs.pop().unwrap();
        self.free_callee_save = self.saved_free_callee_save.pop().unwrap();
        self.free_stack = self.saved_free_stack.pop().unwrap();

        let prev_offset = self.saved_stack_offsets.pop().unwrap();
        if self.restore_stack_offset(emitter, prev_offset) {
            self.stack_offset = prev_offset;
            self.stack.truncate(self.n_stack_params + (self.stack_offset / -8) as usize);
        }
    }

    pub fn normalize_local(&mut self, emitter: &mut E, local: Local<Location<R>>) -> Local<Location<R>> {
        if let Location::Imm32(n) = local.location() {
            let new_local = Local::new(Location::Imm32(n), Size::S32);
            self.move_to_stack(emitter, new_local.clone());
            new_local
        } else if local.ref_ct() > 1 {
            let reg = if let Location::Reg(reg) = local.location() {
                self.get_free_reg(emitter, &[reg])
            } else {
                self.get_free_reg(emitter, &[])
            };
            match local.location() {
                Location::Reg(old_reg) => {
                    emitter.move_reg_to_reg(local.size(), old_reg, reg);
                },
                Location::Memory(base, offset) => {
                    assert!(base == D::FP);
                    emitter.move_mem_to_reg(local.size(), D::FP, offset, reg)
                },
                _ => {
                    // println!("{:?}", local.location());
                    unreachable!();
                }
            }
            self.new_local_from_reg(reg, local.size())
        } else {
            local
        }
    }

    pub fn restore_local(&mut self, emitter: &mut E, local: Local<Location<R>>, location: Location<R>)
        -> Local<Location<R>> {
        
        let local = self.normalize_local(emitter, local);

        if local.location() == location {
            return local;
        }
        
        // ensure destination is available
        match location {
            Location::Reg(reg) => {
                if let Some(other) = self.regs[reg.into_index()].upgrade() {
                    self.move_to_stack(emitter, other);
                    if reg.is_callee_save() {
                        assert!(reg == self.free_callee_save.pop().unwrap());
                    } else {
                        assert!(reg == self.free_regs.pop().unwrap());
                    }
                }
            },
            Location::Memory(base, offset) => {
                assert!(base == D::FP);
                if let Some(other) = self.stack[self.stack_idx(offset)].upgrade() {
                    self.move_to_stack(emitter, other);
                    assert!(offset == self.free_stack.pop().unwrap());
                }
            },
            _ => {
                unreachable!();
            },
        }

        self.move_data(emitter, local.size(), local.location(), location);
        self.release_location(local.clone());
        local.replace_location(location);
        match location {
            Location::Reg(reg) => {
                self.regs[reg.into_index()] = local.downgrade();
            },
            Location::Memory(base, offset) => {
                assert!(base == D::FP);
                let idx = self.stack_idx(offset);
                self.stack[idx] = local.downgrade();
            },
            _ => {
                unreachable!();
            },
        }

        local
    }

    // assumes the returned reg will be eventually released with Machine::release_location()
    fn get_free_reg(&mut self, emitter: &mut E, dont_use: &[R]) -> R {
        if let Some(reg) = self.free_callee_save.pop() {
            reg
        } else if let Some(reg) = self.free_regs.pop() {
            reg
        } else {
            // better not put all the regs in here or this loop will deadlock!!!
            loop {
                let reg = R::from_index(self.reg_counter).unwrap();
                if reg.is_callee_save() || reg.is_reserved() || dont_use.contains(&reg) {
                    self.reg_counter = (self.reg_counter + 1) % D::REG_COUNT;
                    continue;
                } else {
                    break;
                }
            }
            
            let reg_index = self.reg_counter;
            self.reg_counter = (self.reg_counter + 1) % D::REG_COUNT;
            match self.regs[reg_index].upgrade() {
                Some(loc) => {
                    self.move_to_stack(emitter, loc);
                    return self.get_free_reg(emitter, dont_use);
                },
                _ => {
                    unreachable!();
                },
            }
        }
    }

    // CAREFUL WITH THIS!
    fn steal_reg(&mut self, reg: R) -> R {
        let idx = reg.into_index();
        match self.regs[idx].upgrade() {
            Some(local) => {
                assert!(local.ref_ct() < 1);
                local.replace_location(Location::None);
                self.regs[idx] = WeakLocal::new();
                reg
            },
            None => {
                unreachable!();
            }
        }
    }

    pub fn before_call(&mut self, emitter: &mut E, args: &[Local<Location<R>>]) {
        let stack_arg_ct  = args.len().saturating_sub(D::ARG_REG_COUNT);

        for reg in D::caller_save_regs() {
            if let Some(local) = self.regs[reg.into_index()].upgrade() {
                self.move_to_stack(emitter, local);
            }
        }

        if stack_arg_ct > 0 {
            let count = emitter.grow_stack(stack_arg_ct);
            let inc = if D::STACK_GROWS_DOWN { -(D::WORD_SIZE as i32) } else { D::WORD_SIZE as i32 };
            self.stack_offset += count as i32 * inc;
        }
        self.saved_stack_offsets.push(self.stack_offset);

        // vmctx is always passed as the first argument
        self.move_data(emitter, Size::S64, Location::Reg(D::VMCTX), D::caller_arg_location(0));

        for (n, local) in args.iter().enumerate() {
            self.move_data(emitter, local.size(), local.location(), D::caller_arg_location(n + 1));
        }
    }

    pub fn after_call(&mut self, emitter: &mut E, return_types: &[WpType]) -> SmallVec<[Local<Location<R>>; 1]> {
        let stack_offset = self.saved_stack_offsets.pop().unwrap();
        self.restore_stack_offset(emitter, stack_offset);
        self.stack_offset = stack_offset;
        
        let mut returns: SmallVec<[Local<Location<R>>; 1]> = smallvec![];

        if !return_types.is_empty() {
            assert!(return_types.len() == 1);
            
            match return_types[0] {
                WpType::I32|WpType::I64 => {
                    let size = if return_types[0] == WpType::I32 {Size::S64} else {Size::S64};
                    match D::return_location() {
                        Location::Reg(reg) => {
                            returns.push(self.new_local_from_reg(reg, size));
                        },
                        _ => {
                            unimplemented!();
                        },
                    }
                },
                _ => {
                    unimplemented!();
                },
            }
        }

        returns
    }

    fn imm_too_large(&mut self, local: Local<Location<R>>, max_imm_width: u8) -> bool {
        if let Location::Imm32(val) = local.location() {
            if max_imm_width > 31 {
                return false;
            }
            let mask: u32 = 0xffffffff << max_imm_width;
            if mask & val as u32 != 0 {
                return true;
            }
        }
        return false;
    }

    fn in1_out1(&mut self, emitter: &mut E, rules: In1Out1<R, E>, src: Local<Location<R>>) -> Local<Location<R>> {
        if src.location().is_imm() {
            self.move_to_reg(emitter, src.clone(), &[]);
        }

        return match src.location() {
            Location::Reg(reg) => {
                if let Some(reg_reg) = rules.reg_reg {
                    let reg2 = if src.ref_ct() < 1 {
                        self.steal_reg(reg)
                    } else {
                        self.get_free_reg(emitter, &[reg])
                    };
                    reg_reg(emitter, reg, reg2);
                    self.new_local_from_reg(reg2, rules.size)
                } else {
                    unimplemented!();
                }
            },
            _ => {
                unimplemented!();
            }
        };
    }

    fn in1_out0(&mut self, emitter: &mut E, rules: In1Out0<R, E>, src: Local<Location<R>>) {
        if src.location().is_imm() {
            self.move_to_reg(emitter, src.clone(), &[]);
        }

        return match src.location() {
            Location::Reg(reg) => {
                if let Some(reg_f) = rules.reg {
                    reg_f(emitter, reg);
                } else {
                    unimplemented!();
                }
            },
            Location::Memory(base, offset) => {
                if let Some(reg_f) = rules.reg {
                    let reg = self.move_to_reg(emitter, src, &[base]);
                    reg_f(emitter, reg);
                } else {
                    unimplemented!();
                }
            },
            _ => {
                unimplemented!();
            }
        };
    }

    fn in0_out1(&mut self, emitter: &mut E, rules: In0Out1<R, E>) -> Local<Location<R>> {
        if let Some(reg_f) = rules.reg {
            let reg = self.get_free_reg(emitter, &[]);
            reg_f(emitter, reg);
            self.new_local_from_reg(reg, rules.size)
        } else {
            unimplemented!();
        }
    }

    fn in2_out1(&mut self, emitter: &mut E, rules: In2Out1<R, E>,
        src1: Local<Location<R>>, src2: Local<Location<R>>) -> Local<Location<R>> {
        
        // ensure immediates can be encoded, else move them to registers
        if self.imm_too_large(src1.clone(), rules.max_imm_width) {
            if let Location::Reg(reg) = src2.location() {
                self.move_to_reg(emitter, src1.clone(), &[reg]);
            } else {
                self.move_to_reg(emitter, src1.clone(), &[]);
            }
        }
        if self.imm_too_large(src2.clone(), rules.max_imm_width) {
            if let Location::Reg(reg) = src1.location() {
                self.move_to_reg(emitter, src2.clone(), &[reg]);
            } else {
                self.move_to_reg(emitter, src2.clone(), &[]);
            }
        }

        let has_reg_imm_reg = if let Some(_) = rules.reg_imm_reg {true} else {false};
        
        // if operation is commutative and operands will be valid if flipped, flip them
        let (src1, src2) = if let Location::Imm32(_) = src1.location() {
            if has_reg_imm_reg && rules.commutative {
                (src2, src1)
            } else {
                if let Location::Reg(reg) = src1.location() {
                    self.move_to_reg(emitter, src2.clone(), &[reg]);
                } else {
                    self.move_to_reg(emitter, src2.clone(), &[]);
                }
                (src1, src2)
            }
        } else {
            (src1, src2)
        };
        
        return match (src1.location(), src2.location()) {
            (Location::Reg(reg), Location::Imm32(imm)) => {
                if let Some(reg_imm_reg) = rules.reg_imm_reg {
                    let reg2 = if src1.ref_ct() < 1 {
                        self.steal_reg(reg)
                    } else {
                        self.get_free_reg(emitter, &[reg])
                    };
                    reg_imm_reg(emitter, reg, imm, reg2);
                    self.new_local_from_reg(reg2, rules.size)
                } else if let Some(reg_reg_reg) = rules.reg_reg_reg {
                    let reg2 = self.move_to_reg(emitter, src2.clone(), &[reg]);
                    let reg3 = if src2.ref_ct() < 1 {
                        self.steal_reg(reg2)
                    } else {
                        self.get_free_reg(emitter, &[reg, reg2])
                    };
                    reg_reg_reg(emitter, reg, reg2, reg3);
                    self.new_local_from_reg(reg3, rules.size)
                } else if let Some(reg_imm) = rules.reg_imm {
                    let reg = if src1.ref_ct() < 1 {
                        self.steal_reg(reg)
                    } else {
                        let new_reg = self.get_free_reg(emitter, &[reg]);
                        emitter.move_reg_to_reg(rules.size, reg, new_reg);
                        new_reg
                    };
                    reg_imm(emitter, reg, imm);
                    self.new_local_from_reg(reg, rules.size)
                } else if let Some(reg_reg) = rules.reg_reg {
                    let reg = if src1.ref_ct() < 1 {
                        self.steal_reg(reg)
                    } else {
                        let new_reg = self.get_free_reg(emitter, &[reg]);
                        emitter.move_reg_to_reg(rules.size, reg, new_reg);
                        new_reg
                    };
                    let reg2 = self.move_to_reg(emitter, src2.clone(), &[reg]);
                    reg_reg(emitter, reg, reg2);
                    self.new_local_from_reg(reg, rules.size)
                } else {
                    unimplemented!();
                }
            },
            (Location::Reg(reg1), Location::Reg(reg2)) => {
                if let Some(reg_reg_reg) = rules.reg_reg_reg {
                    let reg3 = if src1.ref_ct() < 1 {
                        self.steal_reg(reg1)
                    } else if src2.ref_ct() < 1 {
                        self.steal_reg(reg2)
                    } else {
                        self.get_free_reg(emitter, &[reg1, reg2])
                    };
                    reg_reg_reg(emitter, reg1, reg2, reg3);
                    self.new_local_from_reg(reg3, rules.size)
                } else if let Some(reg_reg) = rules.reg_reg {
                    let reg1 = if src1.ref_ct() < 1 {
                        self.steal_reg(reg1)
                    } else {
                        let new_reg = self.get_free_reg(emitter, &[reg1, reg2]);
                        emitter.move_reg_to_reg(rules.size, reg1, new_reg);
                        new_reg
                    };
                    reg_reg(emitter, reg1, reg2);
                    self.new_local_from_reg(reg1, rules.size)
                } else if let Some((reg_exact_reg, exact, clobbers)) = rules.reg_exact_reg {
                    for reg in clobbers {
                        if let Some(local) = self.regs[reg.into_index()].upgrade() {
                            if local.is(src1.clone()) || local.is(src2.clone()) {
                                let dont_use: Vec<_> = clobbers.iter().chain(&[exact]).map(|&x|x).collect();
                                let reg = self.get_free_reg(emitter, &dont_use);
                                self.regs[reg.into_index()] = local.downgrade();
                                local.replace_location(Location::Reg(reg));
                            } else {
                                self.move_to_stack(emitter, local);
                            }
                        }
                    }
                    // if the above code worked, src1 and src2 should still both be in registers
                    match (src1.location(), src2.location()) {
                        (Location::Reg(_), Location::Reg(_)) => {},
                        _ => { unreachable!(); }
                    }
                    if reg1 == exact {
                        reg_exact_reg(emitter, self.steal_reg(reg2));
                    } else if rules.commutative && reg2 == exact {
                        reg_exact_reg(emitter, self.steal_reg(reg1));
                    } else {
                        if let Some(local) = self.regs[exact.into_index()].upgrade() {
                            self.move_to_stack(emitter, local);
                        }
                        emitter.move_reg_to_reg(rules.size, reg1, exact);
                    }
                    self.new_local_from_reg(exact, rules.size)
                } else {
                    unimplemented!();
                }
            },
            (Location::Reg(reg), Location::Memory(base, _offset)) => {
                if let Some(reg_reg_reg) = rules.reg_reg_reg {
                    let reg2 = self.move_to_reg(emitter, src2.clone(), &[base, reg]);
                    let reg3 = if src1.ref_ct() < 1 {
                        self.steal_reg(reg)
                    } else if src2.ref_ct() < 1 {
                        self.steal_reg(reg2)
                    } else {
                        self.get_free_reg(emitter, &[reg, reg2])
                    };
                    reg_reg_reg(emitter, reg, reg2, reg3);
                    self.new_local_from_reg(reg3, rules.size)
                } else if let Some(reg_reg) = rules.reg_reg {
                    let reg1 = if src1.ref_ct() < 1 {
                        self.steal_reg(reg)
                    } else {
                        let new_reg = self.get_free_reg(emitter, &[base, reg]);
                        emitter.move_reg_to_reg(rules.size, reg, new_reg);
                        new_reg
                    };
                    let reg2 = self.move_to_reg(emitter, src2.clone(), &[base, reg]);
                    reg_reg(emitter, reg1, reg2);
                    self.new_local_from_reg(reg1, rules.size)
                } else {
                    unimplemented!();
                }
            },
            (Location::Memory(base, offset), Location::Reg(reg)) => {
                if let Some(reg_reg_reg) = rules.reg_reg_reg {
                    let reg1 = self.move_to_reg(emitter, src1.clone(), &[reg, base]);
                    let reg3 = if src1.ref_ct() < 1 {
                        self.steal_reg(reg1)
                    } else if src2.ref_ct() < 1 {
                        self.steal_reg(reg)
                    } else {
                        self.get_free_reg(emitter, &[reg1, reg])
                    };
                    reg_reg_reg(emitter, reg1, reg, reg3);
                    self.new_local_from_reg(reg3, rules.size)
                } else if let Some(reg_reg) = rules.reg_reg {
                    let reg1 = if src1.ref_ct() < 1 {
                        let reg = self.move_to_reg(emitter, src1.clone(), &[reg, base]);
                        self.steal_reg(reg)
                    } else {
                        let reg = self.get_free_reg(emitter, &[reg, base]);
                        emitter.move_mem_to_reg(src1.size(), base, offset, reg);
                        reg
                    };
                    reg_reg(emitter, reg1, reg);
                    self.new_local_from_reg(reg1, rules.size)
                } else {
                    unimplemented!();
                }
            },
            (Location::Memory(base, offset), Location::Imm32(imm)) => {
                if let Some(reg_imm_reg) = rules.reg_imm_reg {
                    let reg1 = self.move_to_reg(emitter, src1, &[base]);
                    let reg2 = self.get_free_reg(emitter, &[reg1]);
                    reg_imm_reg(emitter, reg1, imm, reg2);
                    self.new_local_from_reg(reg2, rules.size)
                } else if let Some(reg_imm) = rules.reg_imm {
                    let reg = if src1.ref_ct() < 1 {
                        let reg = self.move_to_reg(emitter, src1.clone(), &[base]);
                        self.steal_reg(reg)
                    } else {
                        let reg = self.get_free_reg(emitter, &[base]);
                        emitter.move_mem_to_reg(src1.size(), base, offset, reg);
                        reg
                    };
                    reg_imm(emitter, reg, imm);
                    self.new_local_from_reg(reg, rules.size)
                } else {
                    unimplemented!();
                }
            },
            _ => {
                unimplemented!();
            }
        };
    }

    fn in2_out0(&mut self, emitter: &mut E, rules: In2Out0<R, E>,
        src1: Local<Location<R>>, src2: Local<Location<R>>) {
        
        match (src1.location(), src2.location()) {
            (Location::Reg(reg1), Location::Reg(reg2)) => {
                if let Some(reg_reg) = rules.reg_reg {
                    reg_reg(emitter, reg1, reg2);
                } else {
                    unimplemented!();
                }
            },
            (Location::Reg(reg), Location::Imm32(_imm)) => {
                if let Some(reg_reg) = rules.reg_reg {
                    let reg2 = self.move_to_reg(emitter, src2, &[reg]);
                    reg_reg(emitter, reg, reg2);
                } else {
                    unimplemented!();
                }
            },
            _ => {
                unimplemented!();
            }
        }
    }

    pub fn set_return_values(&mut self, emitter: &mut E, locals: &[Local<Location<R>>]) {
        assert!(locals.len() == 1);
        let location = D::return_location();
        let local = locals[0].clone();
        if local.location() == location {
            return;
        }

        self.move_data(emitter, local.size(), local.location(), location);
    }

    pub fn br_depth(&mut self, emitter: &mut E, depth: u32) {
        if depth > 0 {
            let idx = self.saved_stack_offsets.len() - depth as usize;
            self.restore_stack_offset(emitter, self.saved_stack_offsets[idx]);
        }
    }

    fn new_local_from_reg(&mut self, reg: R, sz: Size) -> Local<Location<R>> {
        let local = Local::new(Location::Reg(reg), sz);
        if let Some(_) = self.regs[reg.into_index()].upgrade() {
            assert!(false, "tried to create new local from in-use register");
        }
        self.regs[reg.into_index()] = local.downgrade();
        local
    }
    
    fn move_to_reg(&mut self, emitter: &mut E, loc: Local<Location<R>>, dont_use: &[R]) -> R {
        if let Location::Reg(reg) = loc.location() {
            return reg;
        }
        
        let reg = self.get_free_reg(emitter, dont_use);
        match loc.location() {
            Location::Imm32(n) => {
                emitter.move_imm32_to_reg(loc.size(), n, reg);
            },
            Location::Memory(base, offset) => {
                emitter.move_mem_to_reg(loc.size(), base, offset, reg);
            },
            _ => {
                unreachable!();
            },
        }

        self.release_location(loc.clone());
        self.regs[reg.into_index()] = loc.downgrade();
        loc.replace_location(Location::Reg(reg));

        reg
    }

    fn move_data(&mut self, emitter: &mut E, sz: Size, src: Location<R>, dst: Location<R>) {
        match (src, dst) {
            // imm -> reg
            (Location::Imm32(imm), Location::Reg(reg)) => {
                emitter.move_imm32_to_reg(sz, imm, reg);
            },
            // reg -> reg
            (Location::Reg(reg1), Location::Reg(reg2)) => {
                emitter.move_reg_to_reg(sz, reg1, reg2);
            },
            // mem -> reg
            (Location::Memory(base, offset), Location::Reg(reg)) => {
                emitter.move_mem_to_reg(sz, base, offset, reg);
            },
            // reg -> mem
            (Location::Reg(reg), Location::Memory(base, offset)) => {
                emitter.move_reg_to_mem(sz, reg, base, offset);
            },
            // imm -> mem
            (Location::Imm32(imm), Location::Memory(base, offset)) => {
                emitter.move_imm32_to_mem(sz, imm, base, offset);
            },
            // mem -> mem
            (Location::Memory(base1, offset1), Location::Memory(base2, offset2)) => {
                emitter.move_mem_to_mem(sz, base1, offset1, base2, offset2)
            },
            _ => {
                unreachable!();
            },
        }
    }

    pub fn release_location(&mut self, loc: Local<Location<R>>) {
        match loc.location() {
            Location::Reg(reg) => {
                self.regs[reg.into_index()] = WeakLocal::new();
                if reg.is_callee_save() {
                    self.free_callee_save.push(reg);
                } else {
                    self.free_regs.push(reg);
                }
            },
            Location::Memory(base, offset) => {
                if base == D::FP {
                    let idx = self.stack_idx(offset);
                    self.stack[idx] = WeakLocal::new();
                    self.free_stack.push(offset);
                }
            },
            Location::Imm32(_) => {},
            Location::None => {},
        }
    }
}