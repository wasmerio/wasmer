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

lazy_static! {
    static ref CONSTRUCT_STACK_AND_CALL_WASM: unsafe extern "C" fn (stack_top: *const u8, stack_base: *const u8, ctx: *mut vm::Ctx, target: *const vm::Func) -> u64 = {
        let mut assembler = Assembler::new().unwrap();
        let offset = assembler.offset();
        dynasm!(
            assembler
            ; push r15
            ; push r14
            ; push r13
            ; push r12
            ; push r11
            ; push rbp
            ; mov rbp, rsp

            ; mov r15, rdi
            ; mov r14, rsi
            ; mov r13, rdx
            ; mov r12, rcx

            ; mov rdi, r13 // ctx

            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov rsi, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov rdx, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov rcx, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov r8, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov r9, [r14]
            ; sub r14, 8
            ; cmp r14, r15
            ; jb >stack_ready

            ; mov rax, r14
            ; sub rax, r15
            ; sub rsp, rax
            ; sub rsp, 8
            ; mov rax, QWORD 0xfffffffffffffff0u64 as i64
            ; and rsp, rax
            ; mov rax, rsp
            ; loop_begin:
            ; mov r11, [r14]
            ; mov [rax], r11
            ; sub r14, 8
            ; add rax, 8
            ; cmp r14, r15
            ; jb >stack_ready
            ; jmp <loop_begin

            ; stack_ready:
            ; mov rax, QWORD 0xfffffffffffffff0u64 as i64
            ; and rsp, rax
            ; call r12

            ; mov rsp, rbp
            ; pop rbp
            ; pop r11
            ; pop r12
            ; pop r13
            ; pop r14
            ; pop r15
            ; ret
        );
        let buf = assembler.finalize().unwrap();
        let ret = unsafe { ::std::mem::transmute(buf.ptr(offset)) };
        ::std::mem::forget(buf);
        ret
    };
}

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
    vmctx_location: Option<Location>,
    num_params: usize,
    num_locals: usize,
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
        let index = _func_index.index() - self.func_import_count;
        let ptr = self.code.ptr(self.functions[index].begin_offset);
        let return_ty = self.functions[index].returns.last().cloned();
        let buffer: Vec<u64> = _params.iter().rev().map(|x| {
            match *x {
                Value::I32(x) => x as u32 as u64,
                Value::I64(x) => x as u64,
                Value::F32(x) => f32::to_bits(x) as u64,
                Value::F64(x) => f64::to_bits(x),
            }
        }).collect();
        let ret = unsafe {
            CONSTRUCT_STACK_AND_CALL_WASM(
                buffer.as_ptr() as *const u8,
                buffer.as_ptr().offset(buffer.len() as isize) as *const u8,
                _vmctx,
                ptr as _,
            )
        };
        Ok(if let Some(ty) = return_ty {
            vec![match ty {
                WpType::I32 => Value::I32(ret as i32),
                WpType::I64 => Value::I64(ret as i64),
                WpType::F32 => Value::F32(f32::from_bits(ret as u32)),
                WpType::F64 => Value::F64(f64::from_bits(ret as u64)),
                _ => unreachable!(),
            }]
        } else {
            vec![]
        })
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
            num_locals: 0,
            vmctx_location: None,
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
    fn emit_relaxed_binop(
        a: &mut Assembler,
        m: &mut Machine,
        op: fn(&mut Assembler, Size, Location, Location),
        sz: Size,
        src: Location,
        dst: Location,
    ) {
        enum RelaxMode {
            Direct,
            SrcToGPR,
            DstToGPR,
        }
        let mode = match (src, dst) {
            (Location::Memory(_, _), Location::Memory(_, _)) => RelaxMode::SrcToGPR,
            (_, Location::Imm32(_)) | (_, Location::Imm64(_)) => RelaxMode::DstToGPR,
            (Location::Imm64(_), Location::Memory(_, _)) => RelaxMode::SrcToGPR,
            (Location::Imm64(_), Location::GPR(_)) if (op as *const u8 != Assembler::emit_mov as *const u8) => RelaxMode::SrcToGPR,
            _ => RelaxMode::Direct,
        };

        match mode {
            RelaxMode::SrcToGPR => {
                let temp = m.acquire_temp_gpr().unwrap();
                a.emit_mov(sz, src, Location::GPR(temp));
                op(a, sz, Location::GPR(temp), dst);
                m.release_temp_gpr(temp);
            },
            RelaxMode::DstToGPR => {
                let temp = m.acquire_temp_gpr().unwrap();
                a.emit_mov(sz, dst, Location::GPR(temp));
                op(a, sz, src, Location::GPR(temp));
                m.release_temp_gpr(temp);
            },
            RelaxMode::Direct => {
                op(a, sz, src, dst);
            }
        }
    }

    fn emit_binop_i32(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<(Location, LocalOrTemp)>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        // Using Red Zone here.
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(a, &[WpType::I32], false)[0];

        if loc_a != ret {
            let tmp = m.acquire_temp_gpr().unwrap();
            Self::emit_relaxed_binop(
                a, m, Assembler::emit_mov,
                Size::S32, loc_a, Location::GPR(tmp),
            );
            Self::emit_relaxed_binop(
                a, m, f,
                Size::S32, loc_b, Location::GPR(tmp),
            );
            Self::emit_relaxed_binop(
                a, m, Assembler::emit_mov,
                Size::S32, Location::GPR(tmp), ret,
            );
            m.release_temp_gpr(tmp);
        } else {
            Self::emit_relaxed_binop(
                a, m, f,
                Size::S32, loc_b, ret,
            );
        }

        value_stack.push((ret, LocalOrTemp::Temp));
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
                Self::emit_relaxed_binop(
                    a, m, Assembler::emit_cmp,
                    Size::S32, loc_b, loc_a,
                );
                a.emit_set(c, x);
                a.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(x));
            },
            Location::Memory(_, _) => {
                let tmp = m.acquire_temp_gpr().unwrap();
                Self::emit_relaxed_binop(
                    a, m, Assembler::emit_cmp,
                    Size::S32, loc_b, loc_a,
                );
                a.emit_set(c, tmp);
                a.emit_and(Size::S32, Location::Imm32(0xff), Location::GPR(tmp));
                a.emit_mov(Size::S32, Location::GPR(tmp), ret);
                m.release_temp_gpr(tmp);
            },
            _ => unreachable!()
        }
        value_stack.push((ret, LocalOrTemp::Temp));
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

    fn emit_xcnt_i32(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<(Location, LocalOrTemp)>,
        f: fn(&mut Assembler, Size, Location, Location),
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
        value_stack.push((ret, LocalOrTemp::Temp));
    }

    fn emit_shift_i32(
        a: &mut Assembler,
        m: &mut Machine,
        value_stack: &mut Vec<(Location, LocalOrTemp)>,
        f: fn(&mut Assembler, Size, Location, Location),
    ) {
        let loc_b = get_location_released(a, m, value_stack.pop().unwrap());
        let loc_a = get_location_released(a, m, value_stack.pop().unwrap());
        let ret = m.acquire_locations(a, &[WpType::I32], false)[0];

        a.emit_mov(Size::S32, loc_b, Location::GPR(GPR::RCX));

        if loc_a != ret {
            Self::emit_relaxed_binop(
                a, m, Assembler::emit_mov,
                Size::S32, loc_a, ret
            );
        }

        f(a, Size::S32, Location::GPR(GPR::RCX), ret);
        value_stack.push((ret, LocalOrTemp::Temp));
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
        self.num_params += 1;
        self.num_locals += 1;
        Ok(())
    }

    fn feed_local(&mut self, ty: WpType, n: usize) -> Result<(), CodegenError> {
        self.num_locals += n;
        Ok(())
    }

    fn begin_body(&mut self) -> Result<(), CodegenError> {
        let a = self.assembler.as_mut().unwrap();
        a.emit_push(Size::S64, Location::GPR(GPR::RBP));
        a.emit_mov(Size::S64, Location::GPR(GPR::RSP), Location::GPR(GPR::RBP));
        
        let locations = self.machine.acquire_stack_locations(a, 1 + self.num_locals, false);
        self.vmctx_location = Some(locations[0]);
        self.locals = locations[1..].to_vec();

        a.emit_mov(Size::S64, Self::get_param_location(0), self.vmctx_location.unwrap());

        for i in 0..self.num_params {
            a.emit_mov(Size::S64, Self::get_param_location(i + 1), self.locals[i]);
        }
        for i in self.num_params..self.num_locals {
            a.emit_mov(Size::S32, Location::Imm32(0), self.locals[i]);
        }

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
                let loc = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

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
            }
            Operator::GetLocal { local_index } => {
                let local_index = local_index as usize;
                self.value_stack.push((self.locals[local_index], LocalOrTemp::Local));
            }
            Operator::SetLocal { local_index } => {
                let local_index = local_index as usize;
                let loc = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                Self::emit_relaxed_binop(
                    a, &mut self.machine, Assembler::emit_mov,
                    Size::S64, loc, self.locals[local_index],
                );
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
                self.value_stack.push((ret, LocalOrTemp::Temp));
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
                self.value_stack.push((ret, LocalOrTemp::Temp));
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
                self.value_stack.push((ret, LocalOrTemp::Temp));
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
                self.value_stack.push((ret, LocalOrTemp::Temp));
            }
            Operator::I32And => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_and),
            Operator::I32Or => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_or),
            Operator::I32Xor => Self::emit_binop_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_xor),
            Operator::I32Eq => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Equal),
            Operator::I32Ne => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::NotEqual),
            Operator::I32Eqz => Self::emit_cmpop_i32_dynamic_b(a, &mut self.machine, &mut self.value_stack, Condition::Equal, Location::Imm32(0)),
            Operator::I32Clz => Self::emit_xcnt_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_lzcnt),
            Operator::I32Ctz => Self::emit_xcnt_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_tzcnt),
            Operator::I32Popcnt => Self::emit_xcnt_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_popcnt),
            Operator::I32Shl => Self::emit_shift_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_shl),
            Operator::I32ShrU => Self::emit_shift_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_sar),
            Operator::I32ShrS => Self::emit_shift_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_shr),
            Operator::I32Rotl => Self::emit_shift_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_rol),
            Operator::I32Rotr => Self::emit_shift_i32(a, &mut self.machine, &mut self.value_stack, Assembler::emit_ror),
            Operator::I32LtU => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Below),
            Operator::I32LeU => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::BelowEqual),
            Operator::I32GtU => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Above),
            Operator::I32GeU => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::AboveEqual),
            Operator::I32LtS => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Less),
            Operator::I32LeS => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::LessEqual),
            Operator::I32GtS => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::Greater),
            Operator::I32GeS => Self::emit_cmpop_i32(a, &mut self.machine, &mut self.value_stack, Condition::GreaterEqual),
            Operator::If { ty } => {
                let label_end = a.get_label();
                let label_else = a.get_label();

                let cond = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());

                self.control_stack
                    .push(ControlFrame {
                        label: label_end,
                        loop_like: false,
                        if_else: IfElseState::If(label_else),
                        returns: match ty {
                            WpType::EmptyBlockType => vec![],
                            _ => vec![ty],
                        },
                        value_stack_depth: self.value_stack.len(),
                    });
                Self::emit_relaxed_binop(
                    a, &mut self.machine, Assembler::emit_cmp,
                    Size::S32, Location::Imm32(0), cond,
                );
                a.emit_jmp(Condition::Equal, label_else);
            }
            Operator::Else => {
                let mut frame = self.control_stack.last_mut().unwrap();
                let released: Vec<Location> = self.value_stack.drain(..frame.value_stack_depth)
                    .filter(|&(_, lot)| lot == LocalOrTemp::Temp)
                    .map(|(x, _)| x)
                    .collect();
                self.machine.release_locations(a, &released);

                match frame.if_else {
                    IfElseState::If(label) => {
                        a.emit_jmp(Condition::None, frame.label);
                        a.emit_label(label);
                        frame.if_else = IfElseState::Else;
                    }
                    _ => unreachable!()
                }
            }
            Operator::Select => {
                let cond = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let v_b = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let v_a = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                let ret = self.machine.acquire_locations(a, &[WpType::I64], false)[0];
                self.value_stack.push((ret, LocalOrTemp::Temp));

                let end_label = a.get_label();
                let zero_label = a.get_label();

                Self::emit_relaxed_binop(
                    a, &mut self.machine, Assembler::emit_cmp,
                    Size::S32, Location::Imm32(0), cond,
                );
                a.emit_jmp(Condition::Equal, zero_label);
                if v_a != ret {
                    Self::emit_relaxed_binop(
                        a, &mut self.machine, Assembler::emit_mov,
                        Size::S64, v_a, ret,
                    );
                }
                a.emit_jmp(Condition::None, end_label);
                a.emit_label(zero_label);
                if v_b != ret {
                    Self::emit_relaxed_binop(
                        a, &mut self.machine, Assembler::emit_mov,
                        Size::S64, v_b, ret,
                    );
                }
                a.emit_label(end_label);
            }
            Operator::Block { ty } => {
                self.control_stack
                    .push(ControlFrame {
                        label: a.get_label(),
                        loop_like: false,
                        if_else: IfElseState::None,
                        returns: match ty {
                            WpType::EmptyBlockType => vec![],
                            _ => vec![ty],
                        },
                        value_stack_depth: self.value_stack.len(),
                    });
            }
            Operator::Unreachable => {
                a.emit_ud2();
                self.unreachable_depth = 1;
            }
            Operator::Return => {
                let frame = &self.control_stack[0];
                let has_return = if frame.returns.len() > 0 {
                    assert_eq!(frame.returns.len(), 1);
                    let (loc, _) = *self.value_stack.last().unwrap();
                    a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                    true
                } else {
                    false
                };
                let released: Vec<Location> = self.value_stack.drain(..frame.value_stack_depth)
                    .filter(|&(_, lot)| lot == LocalOrTemp::Temp)
                    .map(|(x, _)| x)
                    .collect();
                self.machine.release_locations(a, &released);
                a.emit_jmp(Condition::None, frame.label);
                self.unreachable_depth = 1;
            }
            Operator::Br { relative_depth } => {
                let frame = &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                let has_return = if frame.returns.len() > 0 {
                    assert_eq!(frame.returns.len(), 1);
                    let (loc, _) = *self.value_stack.last().unwrap();
                    a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                    true
                } else {
                    false
                };
                let released: Vec<Location> = self.value_stack.drain(..frame.value_stack_depth)
                    .filter(|&(_, lot)| lot == LocalOrTemp::Temp)
                    .map(|(x, _)| x)
                    .collect();
                self.machine.release_locations(a, &released);
                a.emit_jmp(Condition::None, frame.label);
                self.unreachable_depth = 1;
            }
            Operator::BrIf { relative_depth }=> {
                let after = a.get_label();
                let cond = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                a.emit_cmp(Size::S32, Location::Imm32(0), cond);
                a.emit_jmp(Condition::Equal, after);

                let frame = &self.control_stack[self.control_stack.len() - 1 - (relative_depth as usize)];
                let has_return = if frame.returns.len() > 0 {
                    assert_eq!(frame.returns.len(), 1);
                    let (loc, _) = *self.value_stack.last().unwrap();
                    a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                    true
                } else {
                    false
                };
                let released: Vec<Location> = self.value_stack[frame.value_stack_depth..].iter()
                    .filter(|&&(_, lot)| lot == LocalOrTemp::Temp)
                    .map(|&(x, _)| x)
                    .collect();
                self.machine.release_locations_keep_state(a, &released);
                a.emit_jmp(Condition::None, frame.label);

                a.emit_label(after);
            }
            Operator::Drop => {
                get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
            }
            Operator::End => {
                let frame = self.control_stack.pop().unwrap();
                if self.control_stack.len() == 0 {
                    if !was_unreachable && self.returns.len() > 0 {
                        let loc = get_location_released(a, &mut self.machine, self.value_stack.pop().unwrap());
                        a.emit_mov(Size::S64, loc, Location::GPR(GPR::RAX));
                    }
                    a.emit_label(frame.label);
                    a.emit_mov(Size::S64, Location::GPR(GPR::RBP), Location::GPR(GPR::RSP));
                    a.emit_pop(Size::S64, Location::GPR(GPR::RBP));
                    a.emit_ret();
                } else {
                    let released: Vec<Location> = self.value_stack.drain(..frame.value_stack_depth)
                        .filter(|&(_, lot)| lot == LocalOrTemp::Temp)
                        .map(|(x, _)| x)
                        .collect();
                    self.machine.release_locations(a, &released);

                    if !frame.loop_like {
                        a.emit_label(frame.label);
                    }

                    if let IfElseState::If(label) = frame.if_else {
                        a.emit_label(label);
                    }

                    if frame.returns.len() > 0 {
                        assert_eq!(frame.returns.len(), 1);
                        let loc = self.machine.acquire_locations(a, &frame.returns, false)[0];
                        a.emit_mov(Size::S64, Location::GPR(GPR::RAX), loc);
                        self.value_stack.push((loc, LocalOrTemp::Temp));
                    }
                }
            }
            _ => {
                panic!("not yet implemented: {:?}", op);
            }
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
