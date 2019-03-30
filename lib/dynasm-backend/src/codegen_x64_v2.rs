#![allow(clippy::forget_copy)] // Used by dynasm.

use super::codegen::*;
use crate::protect_unix;
use byteorder::{ByteOrder, LittleEndian};
use dynasmrt::{
    x64::Assembler, AssemblyOffset, DynamicLabel, DynasmApi, DynasmLabelApi, ExecutableBuffer,
};
use std::cell::RefCell;
use std::ptr::NonNull;
use std::{any::Any, collections::HashMap, sync::Arc};
use wasmer_runtime_core::{
    backend::{FuncResolver, ProtectedCaller, Token, UserTrapper},
    error::{RuntimeError, RuntimeResult},
    memory::MemoryType,
    module::{ModuleInfo, ModuleInner},
    structures::{Map, TypedIndex},
    types::{
        FuncIndex, FuncSig, ImportedMemoryIndex, LocalFuncIndex, LocalGlobalIndex,
        LocalMemoryIndex, LocalOrImport, MemoryIndex, SigIndex, Type, Value,
    },
    units::Pages,
    vm::{self, ImportBacking, LocalGlobal, LocalMemory, LocalTable},
};
use wasmparser::{Operator, Type as WpType};
use crate::machine::*;
use crate::emitter_x64::*;

pub struct X64ModuleCodeGenerator {
    functions: Vec<X64FunctionCode>,
    signatures: Option<Arc<Map<SigIndex, FuncSig>>>,
    function_signatures: Option<Arc<Map<FuncIndex, SigIndex>>>,
    function_labels: Option<HashMap<usize, (DynamicLabel, Option<AssemblyOffset>)>>,
    assembler: Option<Assembler>,
    func_import_count: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum LocalOrTemp {
    Local,
    Temp
}

pub struct X64FunctionCode {
    signatures: Arc<Map<SigIndex, FuncSig>>,
    function_signatures: Arc<Map<FuncIndex, SigIndex>>,

    begin_offset: AssemblyOffset,
    assembler: Option<Assembler>,
    function_labels: Option<HashMap<usize, (DynamicLabel, Option<AssemblyOffset>)>>,
    br_table_data: Option<Vec<Vec<usize>>>,
    returns: Vec<WpType>,
    locals: Vec<Location>,
    num_params: usize,
    value_stack: Vec<(Location, LocalOrTemp)>,
    control_stack: Vec<ControlFrame>,
    machine: Machine,
    unreachable_depth: usize,
}

enum FuncPtrInner {}
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
struct FuncPtr(*const FuncPtrInner);
unsafe impl Send for FuncPtr {}
unsafe impl Sync for FuncPtr {}

pub struct X64ExecutionContext {
    code: ExecutableBuffer,
    functions: Vec<X64FunctionCode>,
    signatures: Arc<Map<SigIndex, FuncSig>>,
    function_signatures: Arc<Map<FuncIndex, SigIndex>>,
    function_pointers: Vec<FuncPtr>,
    _br_table_data: Vec<Vec<usize>>,
    func_import_count: usize,
}

pub struct X64RuntimeResolver {
    function_pointers: Vec<FuncPtr>,
}

#[derive(Debug)]
pub struct ControlFrame {
    pub label: DynamicLabel,
    pub loop_like: bool,
    pub if_else: IfElseState,
    pub returns: Vec<WpType>,
    pub value_stack_depth: usize,
}

#[derive(Debug, Copy, Clone)]
pub enum IfElseState {
    None,
    If(DynamicLabel),
    Else,
}

impl X64ExecutionContext {
    fn get_runtime_resolver(
        &self,
        module_info: &ModuleInfo,
    ) -> Result<X64RuntimeResolver, CodegenError> {
        Ok(X64RuntimeResolver {
            function_pointers: self.function_pointers.clone(),
        })
    }
}

impl FuncResolver for X64RuntimeResolver {
    fn get(
        &self,
        _module: &ModuleInner,
        _local_func_index: LocalFuncIndex,
    ) -> Option<NonNull<vm::Func>> {
        NonNull::new(self.function_pointers[_local_func_index.index() as usize].0 as *mut vm::Func)
    }
}

impl ProtectedCaller for X64ExecutionContext {
    fn call(
        &self,
        _module: &ModuleInner,
        _func_index: FuncIndex,
        _params: &[Value],
        _import_backing: &ImportBacking,
        _vmctx: *mut vm::Ctx,
        _: Token,
    ) -> RuntimeResult<Vec<Value>> {
        unimplemented!()
    }

    fn get_early_trapper(&self) -> Box<dyn UserTrapper> {
        pub struct Trapper;

        impl UserTrapper for Trapper {
            unsafe fn do_early_trap(&self, _data: Box<Any>) -> ! {
                panic!("do_early_trap");
            }
        }

        Box::new(Trapper)
    }
}

impl X64ModuleCodeGenerator {
    pub fn new() -> X64ModuleCodeGenerator {
        let mut assembler = Assembler::new().unwrap();

        X64ModuleCodeGenerator {
            functions: vec![],
            signatures: None,
            function_signatures: None,
            function_labels: Some(HashMap::new()),
            assembler: Some(assembler),
            func_import_count: 0,
        }
    }
}

impl ModuleCodeGenerator<X64FunctionCode, X64ExecutionContext, X64RuntimeResolver>
    for X64ModuleCodeGenerator
{
    fn check_precondition(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn next_function(&mut self) -> Result<&mut X64FunctionCode, CodegenError> {
        let (mut assembler, mut function_labels, br_table_data) = match self.functions.last_mut() {
            Some(x) => (
                x.assembler.take().unwrap(),
                x.function_labels.take().unwrap(),
                x.br_table_data.take().unwrap(),
            ),
            None => (
                self.assembler.take().unwrap(),
                self.function_labels.take().unwrap(),
                vec![],
            ),
        };
        let begin_offset = assembler.offset();
        let begin_label_info = function_labels
            .entry(self.functions.len() + self.func_import_count)
            .or_insert_with(|| (assembler.new_dynamic_label(), None));

        begin_label_info.1 = Some(begin_offset);
        let begin_label = begin_label_info.0;

        dynasm!(
            assembler
            ; => begin_label
            //; int 3
        );
        let code = X64FunctionCode {
            signatures: self.signatures.as_ref().unwrap().clone(),
            function_signatures: self.function_signatures.as_ref().unwrap().clone(),

            begin_offset: begin_offset,
            assembler: Some(assembler),
            function_labels: Some(function_labels),
            br_table_data: Some(br_table_data),
            returns: vec![],
            locals: vec![],
            num_params: 0,
            value_stack: vec! [],
            control_stack: vec! [],
            machine: Machine::new(),
            unreachable_depth: 0,
        };
        self.functions.push(code);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(
        mut self,
        module_info: &ModuleInfo,
    ) -> Result<(X64ExecutionContext, X64RuntimeResolver), CodegenError> {
        let (assembler, mut br_table_data) = match self.functions.last_mut() {
            Some(x) => (x.assembler.take().unwrap(), x.br_table_data.take().unwrap()),
            None => {
                return Err(CodegenError {
                    message: "no function",
                });
            }
        };
        let output = assembler.finalize().unwrap();

        for table in &mut br_table_data {
            for entry in table {
                *entry = output.ptr(AssemblyOffset(*entry)) as usize;
            }
        }

        let function_labels = if let Some(x) = self.functions.last() {
            x.function_labels.as_ref().unwrap()
        } else {
            self.function_labels.as_ref().unwrap()
        };
        let mut out_labels: Vec<FuncPtr> = vec![];

        for i in 0..function_labels.len() {
            let (_, offset) = match function_labels.get(&i) {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: "label not found",
                    });
                }
            };
            let offset = match offset {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: "offset is none",
                    });
                }
            };
            out_labels.push(FuncPtr(output.ptr(*offset) as _));
        }

        let ctx = X64ExecutionContext {
            code: output,
            functions: self.functions,
            _br_table_data: br_table_data,
            func_import_count: self.func_import_count,
            signatures: match self.signatures {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: "no signatures",
                    });
                }
            },
            function_pointers: out_labels,
            function_signatures: match self.function_signatures {
                Some(x) => x,
                None => {
                    return Err(CodegenError {
                        message: "no function signatures",
                    });
                }
            },
        };
        let resolver = ctx.get_runtime_resolver(module_info)?;

        Ok((ctx, resolver))
    }

    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError> {
        self.signatures = Some(Arc::new(signatures));
        Ok(())
    }

    fn feed_function_signatures(
        &mut self,
        assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError> {
        self.function_signatures = Some(Arc::new(assoc));
        Ok(())
    }

    fn feed_import_function(&mut self) -> Result<(), CodegenError> {
        /*
        let labels = match self.function_labels.as_mut() {
            Some(x) => x,
            None => {
                return Err(CodegenError {
                    message: "got function import after code",
                });
            }
        };
        let id = labels.len();

        let assembler = self.assembler.as_mut().unwrap();

        let offset = assembler.offset();

        let label = X64FunctionCode::emit_native_call(
            self.assembler.as_mut().unwrap(),
            invoke_import,
            0,
            id,
        );
        labels.insert(id, (label, Some(offset)));

        self.func_import_count += 1;

        Ok(())
        */
        unimplemented!()
    }
}

impl X64FunctionCode {
    fn emit_relaxed_binop<F: FnOnce(&mut Assembler, Size, Location, Location)> (
        a: &mut Assembler,
        m: &mut Machine,
        op: F,
        sz: Size,
        src: Location,
        dst: Location,
    ) {
        match (src, dst) {
            (Location::Memory(_, _), Location::Memory(_, _)) => {
                let temp = m.acquire_temp_gpr().unwrap();
                a.emit_mov(sz, src, Location::GPR(temp));
                op(a, sz, Location::GPR(temp), dst);
                m.release_temp_gpr(temp);
            },
            _ => {
                op(a, sz, src, dst);
            }
        }
    }

    fn emit_binop_i32<F: FnOnce(&mut Assembler, Size, Location, Location)>(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<(Location, LocalOrTemp)>,
        f: F,
    ) {
        // Using Red Zone here.
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(a, &[WpType::I32], false)[0];
        assert_eq!(loc_a, ret);
        Self::emit_relaxed_binop(
            a, m, f,
            Size::S32, loc_b, ret,
        );
    }

    fn emit_cmpop_i32_dynamic_b(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<(Location, LocalOrTemp)>,
        c: Condition,
        loc_b: Location,
    ) {
        // Using Red Zone here.
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(a, &[WpType::I32], false)[0];
        match ret {
            Location::GPR(x) => {
                a.emit_xor(Size::S32, Location::GPR(x), Location::GPR(x));
                Self::emit_relaxed_binop(
                    a, m, Assembler::emit_cmp,
                    Size::S32, loc_b, loc_a,
                );
                a.emit_set(c, x);
            },
            Location::Memory(_, _) => {
                let tmp = m.acquire_temp_gpr().unwrap();
                a.emit_xor(Size::S32, Location::GPR(tmp), Location::GPR(tmp));
                Self::emit_relaxed_binop(
                    a, m, Assembler::emit_cmp,
                    Size::S32, loc_b, loc_a,
                );
                a.emit_set(c, tmp);
                a.emit_mov(Size::S32, Location::GPR(tmp), ret);
                m.release_temp_gpr(tmp);
            },
            _ => unreachable!()
        }
    }

    fn emit_cmpop_i32(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<(Location, LocalOrTemp)>,
        c: Condition,
    ) {
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        Self::emit_cmpop_i32_dynamic_b(a, m, value_stack, c, loc_b);
    }

    fn emit_xcnt_i32<F: FnOnce(&mut Assembler, Size, Location, Location)>(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<(Location, LocalOrTemp)>,
        f: F,
    ) {
        let loc = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(a, &[WpType::I32], false)[0];
        if let Location::Imm32(x) = loc {
            let tmp = m.acquire_temp_gpr().unwrap();
            a.emit_mov(Size::S32, loc, Location::GPR(tmp));
            if let Location::Memory(_, _) = ret {
                let out_tmp = m.acquire_temp_gpr().unwrap();
                f(a, Size::S32, Location::GPR(tmp), Location::GPR(out_tmp));
                a.emit_mov(Size::S32, Location::GPR(out_tmp), ret);
                m.release_temp_gpr(out_tmp);
            } else {
                f(a, Size::S32, Location::GPR(tmp), ret);
            }
            m.release_temp_gpr(tmp);
        } else {
            if let Location::Memory(_, _) = ret {
                let out_tmp = m.acquire_temp_gpr().unwrap();
                f(a, Size::S32, loc, Location::GPR(out_tmp));
                a.emit_mov(Size::S32, Location::GPR(out_tmp), ret);
                m.release_temp_gpr(out_tmp);
            } else {
                f(a, Size::S32, loc, ret);
            }
        }
    }

    fn get_param_location(
        idx: usize
    ) -> Location {
        match idx {
            0 => Location::GPR(GPR::RDI),
            1 => Location::GPR(GPR::RSI),
            2 => Location::GPR(GPR::RDX),
            3 => Location::GPR(GPR::RCX),
            4 => Location::GPR(GPR::R8),
            5 => Location::GPR(GPR::R9),
            _ => Location::Memory(GPR::RBP, (16 + (idx - 6) * 8) as i32),
        }
    }
}

impl FunctionCodeGenerator for X64FunctionCode {
    fn feed_return(&mut self, ty: WpType) -> Result<(), CodegenError> {
        self.returns.push(ty);
        Ok(())
    }

    fn feed_param(&mut self, ty: WpType) -> Result<(), CodegenError> {
        // The first parameter location is reserved for vmctx.
        self.locals.push(Self::get_param_location(
            self.num_params + 1
        ));
        self.num_params += 1;
        Ok(())
    }

    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError> {
        let a = self.assembler.as_mut().unwrap();
        self.machine.acquire_stack_locations(a, n, true);
        Ok(())
    }

    fn begin_body(&mut self) -> Result<(), CodegenError> {
        let a = self.assembler.as_mut().unwrap();
        self.control_stack.push(ControlFrame {
            label: a.get_label(),
            loop_like: false,
            if_else: IfElseState::None,
            returns: self.returns.clone(),
            value_stack_depth: 0,
        });
        Ok(())
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        let a = self.assembler.as_mut().unwrap();
        a.emit_ud2();
        Ok(())
    }
    
    fn feed_opcode(&mut self, op: Operator, module_info: &ModuleInfo) -> Result<(), CodegenError> {
        let was_unreachable;

        if self.unreachable_depth > 0 {
            was_unreachable = true;
            match op {
                Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                    self.unreachable_depth += 1;
                }
                Operator::End => {
                    self.unreachable_depth -= 1;
                }
                Operator::Else => {
                    // We are in a reachable true branch
                    if self.unreachable_depth == 1 {
                        if let Some(IfElseState::If(_)) = self
                            .control_stack
                            .last()
                            .map(|x| x.if_else)
                        {
                            self.unreachable_depth -= 1;
                        }
                    }
                }
                _ => {}
            }
            if self.unreachable_depth > 0 {
                return Ok(());
            }
        } else {
            was_unreachable = false;
        }

        let a = self.assembler.as_mut().unwrap();
        match op {
            Operator::GetGlobal { global_index } => {
                let mut global_index = global_index as usize;
                let loc = self.machine.acquire_locations(
                    a,
                    &[type_to_wp_type(
                        module_info.globals[LocalGlobalIndex::new(global_index)]
                            .desc
                            .ty,
                    )],
                    false
                )[0];
                self.value_stack.push((loc, LocalOrTemp::Temp));

                let tmp = self.machine.acquire_temp_gpr().unwrap();

                if global_index < module_info.imported_globals.len() {
                    a.emit_mov(Size::S64, Location::Memory(GPR::RDI, vm::Ctx::offset_imported_globals() as i32), Location::GPR(tmp));
                } else {
                    global_index -= module_info.imported_globals.len();
                    assert!(global_index < module_info.globals.len());
                    a.emit_mov(Size::S64, Location::Memory(GPR::RDI, vm::Ctx::offset_globals() as i32), Location::GPR(tmp));
                }
                a.emit_mov(Size::S64, Location::Memory(tmp, (global_index as i32) * 8), Location::GPR(tmp));
                Self::emit_relaxed_binop(
                    a, &mut self.machine, Assembler::emit_mov,
                    Size::S64, Location::Memory(tmp, LocalGlobal::offset_data() as i32), loc
                );

                self.machine.release_temp_gpr(tmp);
            }
            Operator::SetGlobal { global_index } => {
                let mut global_index = global_index as usize;
                let (loc, local_or_temp) = self.value_stack.pop().unwrap();

                let tmp = self.machine.acquire_temp_gpr().unwrap();

                if global_index < module_info.imported_globals.len() {
                    a.emit_mov(Size::S64, Location::Memory(GPR::RDI, vm::Ctx::offset_imported_globals() as i32), Location::GPR(tmp));
                } else {
                    global_index -= module_info.imported_globals.len();
                    assert!(global_index < module_info.globals.len());
                    a.emit_mov(Size::S64, Location::Memory(GPR::RDI, vm::Ctx::offset_globals() as i32), Location::GPR(tmp));
                }
                a.emit_mov(Size::S64, Location::Memory(tmp, (global_index as i32) * 8), Location::GPR(tmp));
                Self::emit_relaxed_binop(
                    a, &mut self.machine, Assembler::emit_mov,
                    Size::S64, loc, Location::Memory(tmp, LocalGlobal::offset_data() as i32)
                );

                self.machine.release_temp_gpr(tmp);
                if local_or_temp == LocalOrTemp::Temp {
                    self.machine.release_locations(a, &[loc]);
                }
            }
            Operator::GetLocal { local_index } => {
                let local_index = local_index as usize;
                self.value_stack.push((self.locals[local_index], LocalOrTemp::Local));
            }
            Operator::SetLocal { local_index } => {
                let local_index = local_index as usize;
                let (loc, local_or_temp) = self.value_stack.pop().unwrap();

                Self::emit_relaxed_binop(
                    a, &mut self.machine, Assembler::emit_mov,
                    Size::S64, loc, self.locals[local_index],
                );
                if local_or_temp == LocalOrTemp::Temp {
                    self.machine.release_locations(a, &[loc]);
                }
            }
            Operator::TeeLocal { local_index } => {
                let local_index = local_index as usize;
                let (loc, _) = *self.value_stack.last().unwrap();

                Self::emit_relaxed_binop(
                    a, &mut self.machine, Assembler::emit_mov,
                    Size::S64, loc, self.locals[local_index],
                );
            }
            Operator::I32Const { value } => self.value_stack.push((Location::Imm32(value as u32), LocalOrTemp::Temp)),
            Operator::I32Add => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_add),
            Operator::I32Sub => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_sub),
            Operator::I32Mul => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_imul),
            Operator::I32DivU => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(a, &[WpType::I32], false)[0];
                a.emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                a.emit_xor(Size::S32, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX));
                a.emit_div(Size::S32, loc_b);
                a.emit_mov(Size::S32, Location::GPR(GPR::RAX), ret);
            }
            Operator::I32DivS => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(a, &[WpType::I32], false)[0];
                a.emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                a.emit_xor(Size::S32, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX));
                a.emit_idiv(Size::S32, loc_b);
                a.emit_mov(Size::S32, Location::GPR(GPR::RAX), ret);
            }
            Operator::I32RemU => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(a, &[WpType::I32], false)[0];
                a.emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                a.emit_xor(Size::S32, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX));
                a.emit_div(Size::S32, loc_b);
                a.emit_mov(Size::S32, Location::GPR(GPR::RDX), ret);
            }
            Operator::I32RemS => {
                // We assume that RAX and RDX are temporary registers here.
                let loc_b = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let loc_a = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(a, &[WpType::I32], false)[0];
                a.emit_mov(Size::S32, loc_a, Location::GPR(GPR::RAX));
                a.emit_xor(Size::S32, Location::GPR(GPR::RDX), Location::GPR(GPR::RDX));
                a.emit_idiv(Size::S32, loc_b);
                a.emit_mov(Size::S32, Location::GPR(GPR::RDX), ret);
            }
            Operator::I32And => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_and),
            Operator::I32Or => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_or),
            Operator::I32Xor => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_xor),
            Operator::I32Eq => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Equal),
            Operator::I32Ne => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::NotEqual),
            Operator::I32LtU => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Below),
            Operator::I32LeU => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::BelowEqual),
            Operator::I32GtU => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Above),
            Operator::I32GeU => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::AboveEqual),
            Operator::I32LtS => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Less),
            Operator::I32LeS => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::LessEqual),
            Operator::I32GtS => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Greater),
            Operator::I32GeS => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::GreaterEqual),
            Operator::I32Eqz => Self::emit_cmpop_i32_dynamic_b(a, &mut self.machine, &mut self.value_stack, Condition::Equal, Location::Imm32(0)),
            Operator::I32Clz => Self::emit_xcnt_i32(a, &mut self.machine, &mut self.value_stack, Emitter::emit_lzcnt),
            Operator::I32Ctz => Self::emit_xcnt_i32(a, &mut self.machine, &mut self.value_stack, Emitter::emit_tzcnt),
            Operator::I32Popcnt => Self::emit_xcnt_i32(a, &mut self.machine, &mut self.value_stack, Emitter::emit_popcnt),
            _ => unimplemented!()
        }

        Ok(())
    }
}

fn type_to_wp_type(ty: Type) -> WpType {
    match ty {
        Type::I32 => WpType::I32,
        Type::I64 => WpType::I64,
        Type::F32 => WpType::F32,
        Type::F64 => WpType::F64,
    }
}

fn get_location_released(a: &mut Assembler, m: &mut Machine, (loc, lot): (Location, LocalOrTemp)) -> Location {
    if lot == LocalOrTemp::Temp {
        m.release_locations(a, &[loc]);
    }
    loc
}
