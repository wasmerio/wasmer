use super::{
    intrinsics::{
        func_type_to_llvm, tbaa_label, type_to_llvm, type_to_llvm_ptr, CtxType, GlobalCache,
        Intrinsics, MemoryCache,
    },
    read_info::blocktype_to_type,
    // stackmap::{StackmapEntry, StackmapEntryKind, StackmapRegistry, ValueSemantic},
    state::{ControlFrame, ExtraInfo, IfElseState, State},
    // LLVMBackendConfig, LLVMCallbacks,
};
use inkwell::{
    builder::Builder,
    context::Context,
    module::{Linkage, Module},
    passes::PassManager,
    //targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple},
    targets::FileType,
    types::{BasicType, BasicTypeEnum, FloatMathType, IntType, PointerType, VectorType},
    values::{
        BasicValue, BasicValueEnum, FloatValue, FunctionValue, IntValue, PhiValue, PointerValue,
        VectorValue,
    },
    AddressSpace,
    // OptimizationLevel,
    AtomicOrdering,
    AtomicRMWBinOp,
    FloatPredicate,
    IntPredicate,
};
use smallvec::SmallVec;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::num::TryFromIntError;

use crate::config::LLVMConfig;
use wasm_common::entity::{PrimaryMap, SecondaryMap};
use wasm_common::{
    FunctionIndex, FunctionType, GlobalIndex, LocalFunctionIndex, MemoryIndex, MemoryType,
    Mutability, SignatureIndex, TableIndex, Type,
};
use wasmer_compiler::wasmparser::{self, BinaryReader, MemoryImmediate, Operator};
use wasmer_compiler::{
    to_wasm_error, wasm_unsupported, Addend, CodeOffset, CompileError, CompiledFunction,
    CompiledFunctionFrameInfo, CustomSection, CustomSectionProtection, CustomSections,
    FunctionAddressMap, FunctionBody, FunctionBodyData, InstructionAddressMap, Relocation,
    RelocationKind, RelocationTarget, SectionBody, SectionIndex, SourceLoc, WasmResult,
};
use wasmer_runtime::libcalls::LibCall;
use wasmer_runtime::{
    MemoryPlan, MemoryStyle, ModuleInfo, TablePlan, VMBuiltinFunctionIndex, VMOffsets,
};

// TODO: debugging
use std::fs;
use std::io::Write;

use wasm_common::entity::entity_impl;
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ElfSectionIndex(u32);
entity_impl!(ElfSectionIndex);
impl ElfSectionIndex {
    pub fn is_undef(&self) -> bool {
        self.as_u32() == goblin::elf::section_header::SHN_UNDEF
    }

    pub fn from_usize(value: usize) -> Result<Self, CompileError> {
        match u32::try_from(value) {
            Err(_) => Err(CompileError::Codegen(format!(
                "elf section index {} does not fit in 32 bits",
                value
            ))),
            Ok(value) => Ok(ElfSectionIndex::from_u32(value)),
        }
    }

    pub fn as_usize(&self) -> usize {
        self.as_u32() as usize
    }
}

// TODO
fn wptype_to_type(ty: wasmparser::Type) -> WasmResult<Type> {
    match ty {
        wasmparser::Type::I32 => Ok(Type::I32),
        wasmparser::Type::I64 => Ok(Type::I64),
        wasmparser::Type::F32 => Ok(Type::F32),
        wasmparser::Type::F64 => Ok(Type::F64),
        wasmparser::Type::V128 => Ok(Type::V128),
        wasmparser::Type::AnyRef => Ok(Type::AnyRef),
        wasmparser::Type::AnyFunc => Ok(Type::FuncRef),
        ty => Err(wasm_unsupported!(
            "wptype_to_irtype: parser wasm type {:?}",
            ty
        )),
    }
}

pub struct FuncTranslator {
    ctx: Context,
}

fn const_zero<'ctx>(ty: BasicTypeEnum<'ctx>) -> BasicValueEnum<'ctx> {
    match ty {
        BasicTypeEnum::ArrayType(ty) => ty.const_zero().as_basic_value_enum(),
        BasicTypeEnum::FloatType(ty) => ty.const_zero().as_basic_value_enum(),
        BasicTypeEnum::IntType(ty) => ty.const_zero().as_basic_value_enum(),
        BasicTypeEnum::PointerType(ty) => ty.const_zero().as_basic_value_enum(),
        BasicTypeEnum::StructType(ty) => ty.const_zero().as_basic_value_enum(),
        BasicTypeEnum::VectorType(ty) => ty.const_zero().as_basic_value_enum(),
    }
}

impl FuncTranslator {
    pub fn new() -> Self {
        Self {
            ctx: Context::create(),
        }
    }

    pub fn translate(
        &mut self,
        wasm_module: &ModuleInfo,
        local_func_index: &LocalFunctionIndex,
        function_body: &FunctionBodyData,
        config: &LLVMConfig,
        memory_plans: &PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: &PrimaryMap<TableIndex, TablePlan>,
        func_names: &SecondaryMap<FunctionIndex, String>,
    ) -> Result<(CompiledFunction, CustomSections), CompileError> {
        let func_index = wasm_module.func_index(*local_func_index);
        let func_name = &func_names[func_index];
        let module_name = match wasm_module.name.as_ref() {
            None => format!("<anonymous module> function {}", func_name),
            Some(module_name) => format!("module {} function {}", module_name, func_name),
        };
        let mut module = self.ctx.create_module(module_name.as_str());

        let target_triple = config.target_triple();
        let target_machine = config.target_machine();
        module.set_triple(&target_triple);
        module.set_data_layout(&target_machine.get_target_data().get_data_layout());
        let wasm_fn_type = wasm_module
            .signatures
            .get(wasm_module.functions[func_index])
            .unwrap();

        let intrinsics = Intrinsics::declare(&module, &self.ctx);
        let func_type = func_type_to_llvm(&self.ctx, &intrinsics, wasm_fn_type);

        let func = module.add_function(&func_name, func_type, Some(Linkage::External));
        // TODO: mark vmctx align 16
        // TODO: figure out how many bytes long vmctx is, and mark it dereferenceable. (no need to mark it nonnull once we do this.)
        // TODO: mark vmctx nofree
        func.set_personality_function(intrinsics.personality);
        func.as_global_value().set_section(".wasmer_function");

        let entry = self.ctx.append_basic_block(func, "entry");
        let start_of_code = self.ctx.append_basic_block(func, "start_of_code");
        let return_ = self.ctx.append_basic_block(func, "return");
        let cache_builder = self.ctx.create_builder();
        let builder = self.ctx.create_builder();
        cache_builder.position_at_end(entry);
        let br = cache_builder.build_unconditional_branch(start_of_code);
        cache_builder.position_before(&br);
        builder.position_at_end(start_of_code);

        let mut state = State::new();
        builder.position_at_end(return_);
        let phis: SmallVec<[PhiValue; 1]> = wasm_fn_type
            .results()
            .iter()
            .map(|&wasm_ty| type_to_llvm(&intrinsics, wasm_ty))
            .map(|ty| builder.build_phi(ty, ""))
            .collect();
        state.push_block(return_, phis);
        builder.position_at_end(start_of_code);

        let mut reader =
            BinaryReader::new_with_offset(function_body.data, function_body.module_offset);

        let mut params = vec![];
        for idx in 0..wasm_fn_type.params().len() {
            let ty = wasm_fn_type.params()[idx];
            let ty = type_to_llvm(&intrinsics, ty);
            let value = func
                .get_nth_param((idx as u32).checked_add(1).unwrap())
                .unwrap();
            // TODO: don't interleave allocas and stores.
            let alloca = cache_builder.build_alloca(ty, "param");
            cache_builder.build_store(alloca, value);
            params.push(alloca);
        }

        let mut locals = vec![];
        let num_locals = reader.read_local_count().map_err(to_wasm_error)?;
        for _ in 0..num_locals {
            let mut counter = 0;
            let (count, ty) = reader
                .read_local_decl(&mut counter)
                .map_err(to_wasm_error)?;
            let ty = wptype_to_type(ty)?;
            let ty = type_to_llvm(&intrinsics, ty);
            // TODO: don't interleave allocas and stores.
            for _ in 0..count {
                let alloca = cache_builder.build_alloca(ty, "local");
                cache_builder.build_store(alloca, const_zero(ty));
                locals.push(alloca);
            }
        }

        let mut params_locals = params.clone();
        params_locals.extend(locals.iter().cloned());

        let mut fcg = LLVMFunctionCodeGenerator {
            context: &self.ctx,
            builder,
            intrinsics: &intrinsics,
            state,
            function: func,
            locals: params_locals,
            ctx: CtxType::new(wasm_module, &func, &cache_builder),
            unreachable_depth: 0,
            memory_plans,
            table_plans,
            module: &module,
            // TODO: pointer width
            vmoffsets: VMOffsets::new(8, &wasm_module),
            wasm_module,
            func_names,
        };

        while fcg.state.has_control_frames() {
            let pos = reader.current_position() as u32;
            let op = reader.read_operator().map_err(to_wasm_error)?;
            fcg.translate_operator(op, wasm_module, pos)?;
        }

        // TODO: use phf?
        let mut libcalls = HashMap::new();
        libcalls.insert("vm.exception.trap".to_string(), LibCall::RaiseTrap);
        libcalls.insert("truncf".to_string(), LibCall::TruncF32);
        libcalls.insert("trunc".to_string(), LibCall::TruncF64);
        libcalls.insert("ceilf".to_string(), LibCall::CeilF32);
        libcalls.insert("ceil".to_string(), LibCall::CeilF64);
        libcalls.insert("floorf".to_string(), LibCall::FloorF32);
        libcalls.insert("floor".to_string(), LibCall::FloorF64);
        libcalls.insert("nearbyintf".to_string(), LibCall::NearestF32);
        libcalls.insert("nearbyint".to_string(), LibCall::NearestF64);

        let results = fcg.state.popn_save_extra(wasm_fn_type.results().len())?;
        match results.as_slice() {
            [] => {
                fcg.builder.build_return(None);
            }
            [(one_value, one_value_info)] => {
                let builder = &fcg.builder;
                let one_value = apply_pending_canonicalization(
                    builder,
                    &intrinsics,
                    *one_value,
                    *one_value_info,
                );
                builder.build_return(Some(&builder.build_bitcast(
                    one_value.as_basic_value_enum(),
                    type_to_llvm(&intrinsics, wasm_fn_type.results()[0]),
                    "return",
                )));
            }
            _ => {
                return Err(CompileError::Codegen(
                    "multi-value returns not yet implemented".to_string(),
                ));
            }
        }

        // TODO: debugging
        //module.print_to_stderr();

        // TODO: llvm-callbacks pre-opt-ir
        let pass_manager = PassManager::create(());

        if config.enable_verifier {
            pass_manager.add_verifier_pass();
        }

        pass_manager.add_type_based_alias_analysis_pass();
        pass_manager.add_ipsccp_pass();
        pass_manager.add_prune_eh_pass();
        pass_manager.add_dead_arg_elimination_pass();
        pass_manager.add_function_inlining_pass();
        pass_manager.add_lower_expect_intrinsic_pass();
        pass_manager.add_scalar_repl_aggregates_pass();
        pass_manager.add_instruction_combining_pass();
        pass_manager.add_jump_threading_pass();
        pass_manager.add_correlated_value_propagation_pass();
        pass_manager.add_cfg_simplification_pass();
        pass_manager.add_reassociate_pass();
        pass_manager.add_loop_rotate_pass();
        pass_manager.add_loop_unswitch_pass();
        pass_manager.add_ind_var_simplify_pass();
        pass_manager.add_licm_pass();
        pass_manager.add_loop_vectorize_pass();
        pass_manager.add_instruction_combining_pass();
        pass_manager.add_ipsccp_pass();
        pass_manager.add_reassociate_pass();
        pass_manager.add_cfg_simplification_pass();
        pass_manager.add_gvn_pass();
        pass_manager.add_memcpy_optimize_pass();
        pass_manager.add_dead_store_elimination_pass();
        pass_manager.add_bit_tracking_dce_pass();
        pass_manager.add_instruction_combining_pass();
        pass_manager.add_reassociate_pass();
        pass_manager.add_cfg_simplification_pass();
        pass_manager.add_slp_vectorize_pass();
        pass_manager.add_early_cse_pass();

        pass_manager.run_on(&mut module);

        // TODO: llvm-callbacks llvm post-opt-ir

        let memory_buffer = target_machine
            .write_to_memory_buffer(&mut module, FileType::Object)
            .unwrap();

        // TODO: remove debugging.
        /*
        let mem_buf_slice = memory_buffer.as_slice();
        let mut file = fs::File::create(format!("/home/nicholas/code{}.o", func_name)).unwrap();
        let mut pos = 0;
        while pos < mem_buf_slice.len() {
            pos += file.write(&mem_buf_slice[pos..]).unwrap();
        }
        */

        let mem_buf_slice = memory_buffer.as_slice();
        let object = goblin::Object::parse(&mem_buf_slice).unwrap();
        let elf = match object {
            goblin::Object::Elf(elf) => elf,
            _ => unimplemented!("native object file type not supported"),
        };

        let get_section_name = |section: &goblin::elf::section_header::SectionHeader| {
            if section.sh_name == goblin::elf::section_header::SHN_UNDEF as _ {
                return None;
            }
            let name = elf.strtab.get(section.sh_name);
            if name.is_none() {
                return None;
            }
            let name = name.unwrap();
            if name.is_err() {
                return None;
            }
            Some(name.unwrap())
        };

        // Build up a mapping from a section to its relocation sections.
        let reloc_sections = elf.shdr_relocs.iter().fold(
            HashMap::new(),
            |mut map: HashMap<_, Vec<_>>, (section_index, reloc_section)| {
                let target_section = elf.section_headers[*section_index].sh_info as usize;
                let target_section = ElfSectionIndex::from_usize(target_section).unwrap();
                map.entry(target_section).or_default().push(reloc_section);
                map
            },
        );

        let mut visited: HashSet<ElfSectionIndex> = HashSet::new();
        let mut worklist: Vec<ElfSectionIndex> = Vec::new();
        let mut section_targets: HashMap<ElfSectionIndex, RelocationTarget> = HashMap::new();

        let wasmer_function_index = elf
            .section_headers
            .iter()
            .enumerate()
            .filter(|(_, section)| get_section_name(section) == Some(".wasmer_function"))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if wasmer_function_index.len() != 1 {
            return Err(CompileError::Codegen(format!(
                "found {} sections named .wasmer_function",
                wasmer_function_index.len()
            )));
        }
        let wasmer_function_index = wasmer_function_index[0];
        let wasmer_function_index = ElfSectionIndex::from_usize(wasmer_function_index)?;

        let mut section_to_custom_section = HashMap::new();

        section_targets.insert(
            wasmer_function_index,
            RelocationTarget::LocalFunc(*local_func_index),
        );

        let mut next_custom_section: u32 = 0;
        let mut elf_section_to_target = |elf_section_index: ElfSectionIndex| {
            *section_targets.entry(elf_section_index).or_insert_with(|| {
                let next = SectionIndex::from_u32(next_custom_section);
                section_to_custom_section.insert(elf_section_index, next);
                let target = RelocationTarget::CustomSection(next);
                next_custom_section += 1;
                target
            })
        };

        let section_bytes = |elf_section_index: ElfSectionIndex| {
            let elf_section_index = elf_section_index.as_usize();
            let byte_range = elf.section_headers[elf_section_index].file_range();
            mem_buf_slice[byte_range.start..byte_range.end].to_vec()
        };

        // From elf section index to list of Relocations. Although we use a Vec,
        // the order of relocations is not important.
        let mut relocations: HashMap<ElfSectionIndex, Vec<Relocation>> = HashMap::new();

        // Each iteration of this loop pulls a section and the relocations
        // relocations that apply to it. We begin with the ".wasmer_function"
        // section, and then parse all relocation sections that apply to that
        // section. Those relocations may refer to additional sections which we
        // then add to the worklist until we've visited the closure of
        // everything needed to run the code in ".wasmer_function".
        //
        // `worklist` is the list of sections we have yet to visit. It never
        // contains any duplicates or sections we've already visited. `visited`
        // contains all the sections we've ever added to the worklist in a set
        // so that we can quickly check whether a section is new before adding
        // it to worklist. `section_to_custom_section` is filled in with all
        // the sections we want to include.
        worklist.push(wasmer_function_index);
        visited.insert(wasmer_function_index);
        while let Some(section_index) = worklist.pop() {
            for reloc in reloc_sections
                .get(&section_index)
                .iter()
                .flat_map(|inner| inner.iter().flat_map(|inner2| inner2.iter()))
            {
                let kind = match reloc.r_type {
                    // TODO: these constants are not per-arch, we'll need to
                    // make the whole match per-arch.
                    goblin::elf::reloc::R_X86_64_64 => RelocationKind::Abs8,
                    _ => {
                        return Err(CompileError::Codegen(format!(
                            "unknown ELF relocation {}",
                            reloc.r_type
                        )));
                    }
                };
                let offset = reloc.r_offset as u32;
                let addend = reloc.r_addend.unwrap_or(0);
                let target = reloc.r_sym;
                // TODO: error handling
                let elf_target = elf.syms.get(target).unwrap();
                let elf_target_section = ElfSectionIndex::from_usize(elf_target.st_shndx)?;
                let reloc_target = if elf_target.st_type() == goblin::elf::sym::STT_SECTION {
                    if visited.insert(elf_target_section) {
                        worklist.push(elf_target_section);
                    }
                    elf_section_to_target(elf_target_section)
                } else if elf_target.st_type() == goblin::elf::sym::STT_FUNC
                    && elf_target_section == wasmer_function_index
                {
                    // This is a function referencing its own byte stream.
                    RelocationTarget::LocalFunc(*local_func_index)
                } else if elf_target.st_type() == goblin::elf::sym::STT_NOTYPE
                    && elf_target_section.is_undef()
                {
                    // Not defined in this .o file. Maybe another local function?
                    let name = elf_target.st_name;
                    let name = elf.strtab.get(name).unwrap().unwrap();
                    if let Some((index, _)) =
                        func_names.iter().find(|(_, func_name)| *func_name == name)
                    {
                        let local_index = wasm_module
                            .local_func_index(index)
                            .expect("Relocation to non-local function");
                        RelocationTarget::LocalFunc(local_index)
                    // Maybe a libcall then?
                    } else if let Some(libcall) = libcalls.get(name) {
                        RelocationTarget::LibCall(*libcall)
                    } else {
                        unimplemented!("reference to unknown symbol {}", name);
                    }
                } else {
                    unimplemented!("unknown relocation {:?} with target {:?}", reloc, target);
                };
                relocations
                    .entry(section_index)
                    .or_default()
                    .push(Relocation {
                        kind,
                        reloc_target,
                        offset,
                        addend,
                    });
            }
        }

        let mut custom_sections = section_to_custom_section
            .iter()
            .map(|(elf_section_index, custom_section_index)| {
                (
                    custom_section_index,
                    CustomSection {
                        protection: CustomSectionProtection::Read,
                        bytes: SectionBody::new_with_vec(section_bytes(*elf_section_index)),
                        relocations: relocations
                            .remove_entry(elf_section_index)
                            .map_or(vec![], |(_, v)| v),
                    },
                )
            })
            .collect::<Vec<_>>();
        custom_sections.sort_unstable_by_key(|a| a.0);
        let custom_sections = custom_sections
            .into_iter()
            .map(|(_, v)| v)
            .collect::<PrimaryMap<SectionIndex, _>>();

        let function_body = FunctionBody {
            body: section_bytes(wasmer_function_index),
            unwind_info: None,
        };

        let address_map = FunctionAddressMap {
            instructions: vec![InstructionAddressMap {
                srcloc: SourceLoc::default(),
                code_offset: 0,
                code_len: function_body.body.len(),
            }],
            start_srcloc: SourceLoc::default(),
            end_srcloc: SourceLoc::default(),
            body_offset: 0,
            body_len: function_body.body.len(),
        };

        Ok((
            CompiledFunction {
                body: function_body,
                jt_offsets: SecondaryMap::new(),
                relocations: relocations
                    .remove_entry(&wasmer_function_index)
                    .map_or(vec![], |(_, v)| v),
                frame_info: CompiledFunctionFrameInfo {
                    address_map,
                    traps: vec![],
                },
            },
            custom_sections,
        ))
    }
}

// Create a vector where each lane contains the same value.
fn splat_vector<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    vec_ty: VectorType<'ctx>,
    name: &str,
) -> VectorValue<'ctx> {
    // Use insert_element to insert the element into an undef vector, then use
    // shuffle vector to copy that lane to all lanes.
    builder.build_shuffle_vector(
        builder.build_insert_element(vec_ty.get_undef(), value, intrinsics.i32_zero, ""),
        vec_ty.get_undef(),
        intrinsics.i32_ty.vec_type(vec_ty.get_size()).const_zero(),
        name,
    )
}

impl<'ctx, 'a> LLVMFunctionCodeGenerator<'ctx, 'a> {
    // Convert floating point vector to integer and saturate when out of range.
    // https://github.com/WebAssembly/nontrapping-float-to-int-conversions/blob/master/proposals/nontrapping-float-to-int-conversion/Overview.md
    fn trunc_sat<T: FloatMathType<'ctx>>(
        &self,
        fvec_ty: T,
        ivec_ty: T::MathConvType,
        lower_bound: u64, // Exclusive (lowest representable value)
        upper_bound: u64, // Exclusive (greatest representable value)
        int_min_value: u64,
        int_max_value: u64,
        value: IntValue<'ctx>,
        name: &str,
    ) -> IntValue<'ctx> {
        // a) Compare vector with itself to identify NaN lanes.
        // b) Compare vector with splat of inttofp(upper_bound) to identify
        //    lanes that need to saturate to max.
        // c) Compare vector with splat of inttofp(lower_bound) to identify
        //    lanes that need to saturate to min.
        // d) Use vector select (not shuffle) to pick from either the
        //    splat vector or the input vector depending on whether the
        //    comparison indicates that we have an unrepresentable value. Replace
        //    unrepresentable values with zero.
        // e) Now that the value is safe, fpto[su]i it.
        // f) Use our previous comparison results to replace certain zeros with
        //    int_min or int_max.

        let fvec_ty = fvec_ty.as_basic_type_enum().into_vector_type();
        let ivec_ty = ivec_ty.as_basic_type_enum().into_vector_type();
        let fvec_element_ty = fvec_ty.get_element_type().into_float_type();
        let ivec_element_ty = ivec_ty.get_element_type().into_int_type();

        let is_signed = int_min_value != 0;
        let int_min_value = splat_vector(
            &self.builder,
            self.intrinsics,
            ivec_element_ty
                .const_int(int_min_value, is_signed)
                .as_basic_value_enum(),
            ivec_ty,
            "",
        );
        let int_max_value = splat_vector(
            &self.builder,
            self.intrinsics,
            ivec_element_ty
                .const_int(int_max_value, is_signed)
                .as_basic_value_enum(),
            ivec_ty,
            "",
        );
        let lower_bound = if is_signed {
            self.builder.build_signed_int_to_float(
                ivec_element_ty.const_int(lower_bound, is_signed),
                fvec_element_ty,
                "",
            )
        } else {
            self.builder.build_unsigned_int_to_float(
                ivec_element_ty.const_int(lower_bound, is_signed),
                fvec_element_ty,
                "",
            )
        };
        let upper_bound = if is_signed {
            self.builder.build_signed_int_to_float(
                ivec_element_ty.const_int(upper_bound, is_signed),
                fvec_element_ty,
                "",
            )
        } else {
            self.builder.build_unsigned_int_to_float(
                ivec_element_ty.const_int(upper_bound, is_signed),
                fvec_element_ty,
                "",
            )
        };

        let value = self
            .builder
            .build_bitcast(value, fvec_ty, "")
            .into_vector_value();
        let zero = fvec_ty.const_zero();
        let lower_bound = splat_vector(
            &self.builder,
            self.intrinsics,
            lower_bound.as_basic_value_enum(),
            fvec_ty,
            "",
        );
        let upper_bound = splat_vector(
            &self.builder,
            self.intrinsics,
            upper_bound.as_basic_value_enum(),
            fvec_ty,
            "",
        );
        let nan_cmp = self
            .builder
            .build_float_compare(FloatPredicate::UNO, value, zero, "nan");
        let above_upper_bound_cmp = self.builder.build_float_compare(
            FloatPredicate::OGT,
            value,
            upper_bound,
            "above_upper_bound",
        );
        let below_lower_bound_cmp = self.builder.build_float_compare(
            FloatPredicate::OLT,
            value,
            lower_bound,
            "below_lower_bound",
        );
        let not_representable = self.builder.build_or(
            self.builder.build_or(nan_cmp, above_upper_bound_cmp, ""),
            below_lower_bound_cmp,
            "not_representable_as_int",
        );
        let value = self
            .builder
            .build_select(not_representable, zero, value, "safe_to_convert")
            .into_vector_value();
        let value = if is_signed {
            self.builder
                .build_float_to_signed_int(value, ivec_ty, "as_int")
        } else {
            self.builder
                .build_float_to_unsigned_int(value, ivec_ty, "as_int")
        };
        let value = self
            .builder
            .build_select(above_upper_bound_cmp, int_max_value, value, "")
            .into_vector_value();
        let res = self
            .builder
            .build_select(below_lower_bound_cmp, int_min_value, value, name)
            .into_vector_value();
        self.builder
            .build_bitcast(res, self.intrinsics.i128_ty, "")
            .into_int_value()
    }
}

impl<'ctx, 'a> LLVMFunctionCodeGenerator<'ctx, 'a> {
    // Convert floating point vector to integer and saturate when out of range.
    // https://github.com/WebAssembly/nontrapping-float-to-int-conversions/blob/master/proposals/nontrapping-float-to-int-conversion/Overview.md
    fn trunc_sat_scalar(
        &self,
        int_ty: IntType<'ctx>,
        lower_bound: u64, // Exclusive (lowest representable value)
        upper_bound: u64, // Exclusive (greatest representable value)
        int_min_value: u64,
        int_max_value: u64,
        value: FloatValue<'ctx>,
        name: &str,
    ) -> IntValue<'ctx> {
        // TODO: this is a scalarized version of the process in trunc_sat. Either
        // we should merge with trunc_sat, or we should simplify this function.

        // a) Compare value with itself to identify NaN.
        // b) Compare value inttofp(upper_bound) to identify values that need to
        //    saturate to max.
        // c) Compare value with inttofp(lower_bound) to identify values that need
        //    to saturate to min.
        // d) Use select to pick from either zero or the input vector depending on
        //    whether the comparison indicates that we have an unrepresentable
        //    value.
        // e) Now that the value is safe, fpto[su]i it.
        // f) Use our previous comparison results to replace certain zeros with
        //    int_min or int_max.

        let is_signed = int_min_value != 0;
        let int_min_value = int_ty.const_int(int_min_value, is_signed);
        let int_max_value = int_ty.const_int(int_max_value, is_signed);

        let lower_bound = if is_signed {
            self.builder.build_signed_int_to_float(
                int_ty.const_int(lower_bound, is_signed),
                value.get_type(),
                "",
            )
        } else {
            self.builder.build_unsigned_int_to_float(
                int_ty.const_int(lower_bound, is_signed),
                value.get_type(),
                "",
            )
        };
        let upper_bound = if is_signed {
            self.builder.build_signed_int_to_float(
                int_ty.const_int(upper_bound, is_signed),
                value.get_type(),
                "",
            )
        } else {
            self.builder.build_unsigned_int_to_float(
                int_ty.const_int(upper_bound, is_signed),
                value.get_type(),
                "",
            )
        };

        let zero = value.get_type().const_zero();

        let nan_cmp = self
            .builder
            .build_float_compare(FloatPredicate::UNO, value, zero, "nan");
        let above_upper_bound_cmp = self.builder.build_float_compare(
            FloatPredicate::OGT,
            value,
            upper_bound,
            "above_upper_bound",
        );
        let below_lower_bound_cmp = self.builder.build_float_compare(
            FloatPredicate::OLT,
            value,
            lower_bound,
            "below_lower_bound",
        );
        let not_representable = self.builder.build_or(
            self.builder.build_or(nan_cmp, above_upper_bound_cmp, ""),
            below_lower_bound_cmp,
            "not_representable_as_int",
        );
        let value = self
            .builder
            .build_select(not_representable, zero, value, "safe_to_convert")
            .into_float_value();
        let value = if is_signed {
            self.builder
                .build_float_to_signed_int(value, int_ty, "as_int")
        } else {
            self.builder
                .build_float_to_unsigned_int(value, int_ty, "as_int")
        };
        let value = self
            .builder
            .build_select(above_upper_bound_cmp, int_max_value, value, "")
            .into_int_value();
        let value = self
            .builder
            .build_select(below_lower_bound_cmp, int_min_value, value, name)
            .into_int_value();
        self.builder
            .build_bitcast(value, int_ty, "")
            .into_int_value()
    }
}

fn trap_if_not_representable_as_int<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    context: &'ctx Context,
    function: &FunctionValue<'ctx>,
    lower_bound: u64, // Inclusive (not a trapping value)
    upper_bound: u64, // Inclusive (not a trapping value)
    value: FloatValue,
) {
    let float_ty = value.get_type();
    let int_ty = if float_ty == intrinsics.f32_ty {
        intrinsics.i32_ty
    } else {
        intrinsics.i64_ty
    };

    let lower_bound = builder
        .build_bitcast(int_ty.const_int(lower_bound, false), float_ty, "")
        .into_float_value();
    let upper_bound = builder
        .build_bitcast(int_ty.const_int(upper_bound, false), float_ty, "")
        .into_float_value();

    // The 'U' in the float predicate is short for "unordered" which means that
    // the comparison will compare true if either operand is a NaN. Thus, NaNs
    // are out of bounds.
    let above_upper_bound_cmp =
        builder.build_float_compare(FloatPredicate::UGT, value, upper_bound, "above_upper_bound");
    let below_lower_bound_cmp =
        builder.build_float_compare(FloatPredicate::ULT, value, lower_bound, "below_lower_bound");
    let out_of_bounds = builder.build_or(
        above_upper_bound_cmp,
        below_lower_bound_cmp,
        "out_of_bounds",
    );

    let failure_block = context.append_basic_block(*function, "conversion_failure_block");
    let continue_block = context.append_basic_block(*function, "conversion_success_block");

    builder.build_conditional_branch(out_of_bounds, failure_block, continue_block);
    builder.position_at_end(failure_block);
    let is_nan = builder.build_float_compare(FloatPredicate::UNO, value, value, "is_nan");
    let trap_code = builder.build_select(
        is_nan,
        intrinsics.trap_bad_conversion_to_integer,
        intrinsics.trap_illegal_arithmetic,
        "",
    );
    builder.build_call(intrinsics.throw_trap, &[trap_code], "throw");
    builder.build_unreachable();
    builder.position_at_end(continue_block);
}

fn trap_if_zero_or_overflow<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    context: &'ctx Context,
    function: &FunctionValue<'ctx>,
    left: IntValue,
    right: IntValue,
) {
    let int_type = left.get_type();

    let (min_value, neg_one_value) = if int_type == intrinsics.i32_ty {
        let min_value = int_type.const_int(i32::min_value() as u64, false);
        let neg_one_value = int_type.const_int(-1i32 as u32 as u64, false);
        (min_value, neg_one_value)
    } else if int_type == intrinsics.i64_ty {
        let min_value = int_type.const_int(i64::min_value() as u64, false);
        let neg_one_value = int_type.const_int(-1i64 as u64, false);
        (min_value, neg_one_value)
    } else {
        unreachable!()
    };

    let divisor_is_zero = builder.build_int_compare(
        IntPredicate::EQ,
        right,
        int_type.const_int(0, false),
        "divisor_is_zero",
    );
    let should_trap = builder.build_or(
        divisor_is_zero,
        builder.build_and(
            builder.build_int_compare(IntPredicate::EQ, left, min_value, "left_is_min"),
            builder.build_int_compare(IntPredicate::EQ, right, neg_one_value, "right_is_neg_one"),
            "div_will_overflow",
        ),
        "div_should_trap",
    );

    let should_trap = builder
        .build_call(
            intrinsics.expect_i1,
            &[
                should_trap.as_basic_value_enum(),
                intrinsics.i1_ty.const_int(0, false).as_basic_value_enum(),
            ],
            "should_trap_expect",
        )
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

    let shouldnt_trap_block = context.append_basic_block(*function, "shouldnt_trap_block");
    let should_trap_block = context.append_basic_block(*function, "should_trap_block");
    builder.build_conditional_branch(should_trap, should_trap_block, shouldnt_trap_block);
    builder.position_at_end(should_trap_block);
    let trap_code = builder.build_select(
        divisor_is_zero,
        intrinsics.trap_integer_division_by_zero,
        intrinsics.trap_illegal_arithmetic,
        "",
    );
    builder.build_call(intrinsics.throw_trap, &[trap_code], "throw");
    builder.build_unreachable();
    builder.position_at_end(shouldnt_trap_block);
}

fn trap_if_zero<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    context: &'ctx Context,
    function: &FunctionValue<'ctx>,
    value: IntValue,
) {
    let int_type = value.get_type();
    let should_trap = builder.build_int_compare(
        IntPredicate::EQ,
        value,
        int_type.const_int(0, false),
        "divisor_is_zero",
    );

    let should_trap = builder
        .build_call(
            intrinsics.expect_i1,
            &[
                should_trap.as_basic_value_enum(),
                intrinsics.i1_ty.const_int(0, false).as_basic_value_enum(),
            ],
            "should_trap_expect",
        )
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

    let shouldnt_trap_block = context.append_basic_block(*function, "shouldnt_trap_block");
    let should_trap_block = context.append_basic_block(*function, "should_trap_block");
    builder.build_conditional_branch(should_trap, should_trap_block, shouldnt_trap_block);
    builder.position_at_end(should_trap_block);
    builder.build_call(
        intrinsics.throw_trap,
        &[intrinsics.trap_integer_division_by_zero],
        "throw",
    );
    builder.build_unreachable();
    builder.position_at_end(shouldnt_trap_block);
}

fn v128_into_int_vec<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    info: ExtraInfo,
    int_vec_ty: VectorType<'ctx>,
) -> (VectorValue<'ctx>, ExtraInfo) {
    let (value, info) = if info.has_pending_f32_nan() {
        let value = builder.build_bitcast(value, intrinsics.f32x4_ty, "");
        (
            canonicalize_nans(builder, intrinsics, value),
            info.strip_pending(),
        )
    } else if info.has_pending_f64_nan() {
        let value = builder.build_bitcast(value, intrinsics.f64x2_ty, "");
        (
            canonicalize_nans(builder, intrinsics, value),
            info.strip_pending(),
        )
    } else {
        (value, info)
    };
    (
        builder
            .build_bitcast(value, int_vec_ty, "")
            .into_vector_value(),
        info,
    )
}

fn v128_into_i8x16<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    info: ExtraInfo,
) -> (VectorValue<'ctx>, ExtraInfo) {
    v128_into_int_vec(builder, intrinsics, value, info, intrinsics.i8x16_ty)
}

fn v128_into_i16x8<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    info: ExtraInfo,
) -> (VectorValue<'ctx>, ExtraInfo) {
    v128_into_int_vec(builder, intrinsics, value, info, intrinsics.i16x8_ty)
}

fn v128_into_i32x4<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    info: ExtraInfo,
) -> (VectorValue<'ctx>, ExtraInfo) {
    v128_into_int_vec(builder, intrinsics, value, info, intrinsics.i32x4_ty)
}

fn v128_into_i64x2<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    info: ExtraInfo,
) -> (VectorValue<'ctx>, ExtraInfo) {
    v128_into_int_vec(builder, intrinsics, value, info, intrinsics.i64x2_ty)
}

// If the value is pending a 64-bit canonicalization, do it now.
// Return a f32x4 vector.
fn v128_into_f32x4<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    info: ExtraInfo,
) -> (VectorValue<'ctx>, ExtraInfo) {
    let (value, info) = if info.has_pending_f64_nan() {
        let value = builder.build_bitcast(value, intrinsics.f64x2_ty, "");
        (
            canonicalize_nans(builder, intrinsics, value),
            info.strip_pending(),
        )
    } else {
        (value, info)
    };
    (
        builder
            .build_bitcast(value, intrinsics.f32x4_ty, "")
            .into_vector_value(),
        info,
    )
}

// If the value is pending a 32-bit canonicalization, do it now.
// Return a f64x2 vector.
fn v128_into_f64x2<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    info: ExtraInfo,
) -> (VectorValue<'ctx>, ExtraInfo) {
    let (value, info) = if info.has_pending_f32_nan() {
        let value = builder.build_bitcast(value, intrinsics.f32x4_ty, "");
        (
            canonicalize_nans(builder, intrinsics, value),
            info.strip_pending(),
        )
    } else {
        (value, info)
    };
    (
        builder
            .build_bitcast(value, intrinsics.f64x2_ty, "")
            .into_vector_value(),
        info,
    )
}

fn apply_pending_canonicalization<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
    info: ExtraInfo,
) -> BasicValueEnum<'ctx> {
    if info.has_pending_f32_nan() {
        if value.get_type().is_vector_type()
            || value.get_type() == intrinsics.i128_ty.as_basic_type_enum()
        {
            let ty = value.get_type();
            let value = builder.build_bitcast(value, intrinsics.f32x4_ty, "");
            let value = canonicalize_nans(builder, intrinsics, value);
            builder.build_bitcast(value, ty, "")
        } else {
            canonicalize_nans(builder, intrinsics, value)
        }
    } else if info.has_pending_f64_nan() {
        if value.get_type().is_vector_type()
            || value.get_type() == intrinsics.i128_ty.as_basic_type_enum()
        {
            let ty = value.get_type();
            let value = builder.build_bitcast(value, intrinsics.f64x2_ty, "");
            let value = canonicalize_nans(builder, intrinsics, value);
            builder.build_bitcast(value, ty, "")
        } else {
            canonicalize_nans(builder, intrinsics, value)
        }
    } else {
        value
    }
}

// Replaces any NaN with the canonical QNaN, otherwise leaves the value alone.
fn canonicalize_nans<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    value: BasicValueEnum<'ctx>,
) -> BasicValueEnum<'ctx> {
    let f_ty = value.get_type();
    let canonicalized = if f_ty.is_vector_type() {
        let value = value.into_vector_value();
        let f_ty = f_ty.into_vector_type();
        let zero = f_ty.const_zero();
        let nan_cmp = builder.build_float_compare(FloatPredicate::UNO, value, zero, "nan");
        let canonical_qnan = f_ty
            .get_element_type()
            .into_float_type()
            .const_float(std::f64::NAN);
        let canonical_qnan = splat_vector(
            builder,
            intrinsics,
            canonical_qnan.as_basic_value_enum(),
            f_ty,
            "",
        );
        builder
            .build_select(nan_cmp, canonical_qnan, value, "")
            .as_basic_value_enum()
    } else {
        let value = value.into_float_value();
        let f_ty = f_ty.into_float_type();
        let zero = f_ty.const_zero();
        let nan_cmp = builder.build_float_compare(FloatPredicate::UNO, value, zero, "nan");
        let canonical_qnan = f_ty.const_float(std::f64::NAN);
        builder
            .build_select(nan_cmp, canonical_qnan, value, "")
            .as_basic_value_enum()
    };
    canonicalized
}

impl<'ctx, 'a> LLVMFunctionCodeGenerator<'ctx, 'a> {
    pub fn resolve_memory_ptr(
        &mut self,
        memory_plans: &PrimaryMap<MemoryIndex, MemoryPlan>,
        memarg: &MemoryImmediate,
        ptr_ty: PointerType<'ctx>,
        var_offset: IntValue<'ctx>,
        value_size: usize,
    ) -> Result<PointerValue<'ctx>, CompileError> {
        let builder = &self.builder;
        let intrinsics = &self.intrinsics;
        let context = &self.context;
        let module = &self.module;
        let wasm_module = &self.wasm_module;
        let function = &self.function;
        let ctx = &mut self.ctx;

        let memory_index = MemoryIndex::from_u32(0);

        // Compute the offset into the storage.
        let imm_offset = intrinsics.i64_ty.const_int(memarg.offset as u64, false);
        let var_offset = builder.build_int_z_extend(var_offset, intrinsics.i64_ty, "");
        let offset = builder.build_int_add(var_offset, imm_offset, "");

        // Look up the memory base (as pointer) and bounds (as unsigned integer).
        let base_ptr = match self
            .ctx
            .memory(memory_index, intrinsics, module, memory_plans)
        {
            MemoryCache::Dynamic {
                ptr_to_base_ptr,
                current_length_ptr,
            } => {
                // Bounds check it.
                let current_length = builder.build_load(current_length_ptr, "");
                // TODO: tbaa_label
                let minimum = memory_plans[memory_index].memory.minimum;
                let maximum = memory_plans[memory_index].memory.maximum;
                // If the memory is dynamic, do a bounds check. For static we rely on
                // the size being a multiple of the page size and hitting a guard page.
                let value_size_v = intrinsics.i64_ty.const_int(value_size as u64, false);
                let ptr_in_bounds = if offset.is_const() {
                    let load_offset_end = offset.const_add(value_size_v);
                    let ptr_in_bounds = load_offset_end.const_int_compare(
                        IntPredicate::ULE,
                        intrinsics.i64_ty.const_int(minimum.bytes().0 as u64, false),
                    );
                    if ptr_in_bounds.get_zero_extended_constant() == Some(1) {
                        Some(ptr_in_bounds)
                    } else {
                        None
                    }
                } else {
                    None
                }
                .unwrap_or_else(|| {
                    let load_offset_end = builder.build_int_add(offset, value_size_v, "");

                    let current_length =
                        builder.build_load(current_length_ptr, "").into_int_value();

                    builder.build_int_compare(
                        IntPredicate::ULE,
                        load_offset_end,
                        current_length,
                        "",
                    )
                });
                if !ptr_in_bounds.is_constant_int()
                    || ptr_in_bounds.get_zero_extended_constant().unwrap() != 1
                {
                    // LLVM may have folded this into 'i1 true' in which case we know
                    // the pointer is in bounds. LLVM may also have folded it into a
                    // constant expression, not known to be either true or false yet.
                    // If it's false, unknown-but-constant, or not-a-constant, emit a
                    // runtime bounds check. LLVM may yet succeed at optimizing it away.
                    let ptr_in_bounds = builder
                        .build_call(
                            intrinsics.expect_i1,
                            &[
                                ptr_in_bounds.as_basic_value_enum(),
                                intrinsics.i1_ty.const_int(1, false).as_basic_value_enum(),
                            ],
                            "ptr_in_bounds_expect",
                        )
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_int_value();

                    let in_bounds_continue_block =
                        context.append_basic_block(*function, "in_bounds_continue_block");
                    let not_in_bounds_block =
                        context.append_basic_block(*function, "not_in_bounds_block");
                    builder.build_conditional_branch(
                        ptr_in_bounds,
                        in_bounds_continue_block,
                        not_in_bounds_block,
                    );
                    builder.position_at_end(not_in_bounds_block);
                    builder.build_call(
                        intrinsics.throw_trap,
                        &[intrinsics.trap_memory_oob],
                        "throw",
                    );
                    builder.build_unreachable();
                    builder.position_at_end(in_bounds_continue_block);
                }
                builder.build_load(ptr_to_base_ptr, "").into_pointer_value()
                // TODO: tbaa label
            }
            MemoryCache::Static { base_ptr } => base_ptr,
        };
        let value_ptr = unsafe { builder.build_gep(base_ptr, &[offset], "") };
        Ok(builder
            .build_bitcast(value_ptr, ptr_ty, "")
            .into_pointer_value())
    }
}

/*
fn emit_stack_map<'ctx>(
    intrinsics: &Intrinsics<'ctx>,
    builder: &Builder<'ctx>,
    local_function_id: usize,
    target: &mut StackmapRegistry,
    kind: StackmapEntryKind,
    locals: &[PointerValue],
    state: &State<'ctx>,
    _ctx: &mut CtxType<'ctx>,
    opcode_offset: usize,
) {
    let stackmap_id = target.entries.len();

    let mut params = Vec::with_capacity(2 + locals.len() + state.stack.len());

    params.push(
        intrinsics
            .i64_ty
            .const_int(stackmap_id as u64, false)
            .as_basic_value_enum(),
    );
    params.push(intrinsics.i32_ty.const_int(0, false).as_basic_value_enum());

    let locals: Vec<_> = locals.iter().map(|x| x.as_basic_value_enum()).collect();
    let mut value_semantics: Vec<ValueSemantic> =
        Vec::with_capacity(locals.len() + state.stack.len());

    params.extend_from_slice(&locals);
    value_semantics.extend((0..locals.len()).map(ValueSemantic::WasmLocal));

    params.extend(state.stack.iter().map(|x| x.0));
    value_semantics.extend((0..state.stack.len()).map(ValueSemantic::WasmStack));

    // FIXME: Information needed for Abstract -> Runtime state transform is not fully preserved
    // to accelerate compilation and reduce memory usage. Check this again when we try to support
    // "full" LLVM OSR.

    assert_eq!(params.len(), value_semantics.len() + 2);

    builder.build_call(intrinsics.experimental_stackmap, &params, "");

    target.entries.push(StackmapEntry {
        kind,
        local_function_id,
        local_count: locals.len(),
        stack_count: state.stack.len(),
        opcode_offset,
        value_semantics,
        is_start: true,
    });
}

fn finalize_opcode_stack_map<'ctx>(
    intrinsics: &Intrinsics<'ctx>,
    builder: &Builder<'ctx>,
    local_function_id: usize,
    target: &mut StackmapRegistry,
    kind: StackmapEntryKind,
    opcode_offset: usize,
) {
    let stackmap_id = target.entries.len();
    builder.build_call(
        intrinsics.experimental_stackmap,
        &[
            intrinsics
                .i64_ty
                .const_int(stackmap_id as u64, false)
                .as_basic_value_enum(),
            intrinsics.i32_ty.const_int(0, false).as_basic_value_enum(),
        ],
        "opcode_stack_map_end",
    );
    target.entries.push(StackmapEntry {
        kind,
        local_function_id,
        local_count: 0,
        stack_count: 0,
        opcode_offset,
        value_semantics: vec![],
        is_start: false,
    });
}
*/
fn trap_if_misaligned<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    context: &'ctx Context,
    function: &FunctionValue<'ctx>,
    memarg: &MemoryImmediate,
    ptr: PointerValue<'ctx>,
) {
    let align = match memarg.flags & 3 {
        0 => {
            return; /* No alignment to check. */
        }
        1 => 2,
        2 => 4,
        3 => 8,
        _ => unreachable!("this match is fully covered"),
    };
    let value = builder.build_ptr_to_int(ptr, intrinsics.i64_ty, "");
    let and = builder.build_and(
        value,
        intrinsics.i64_ty.const_int(align - 1, false),
        "misaligncheck",
    );
    let aligned = builder.build_int_compare(IntPredicate::EQ, and, intrinsics.i64_zero, "");
    let aligned = builder
        .build_call(
            intrinsics.expect_i1,
            &[
                aligned.as_basic_value_enum(),
                intrinsics.i1_ty.const_int(1, false).as_basic_value_enum(),
            ],
            "",
        )
        .try_as_basic_value()
        .left()
        .unwrap()
        .into_int_value();

    let continue_block = context.append_basic_block(*function, "aligned_access_continue_block");
    let not_aligned_block = context.append_basic_block(*function, "misaligned_trap_block");
    builder.build_conditional_branch(aligned, continue_block, not_aligned_block);

    builder.position_at_end(not_aligned_block);
    builder.build_call(
        intrinsics.throw_trap,
        &[intrinsics.trap_unaligned_atomic],
        "throw",
    );
    builder.build_unreachable();

    builder.position_at_end(continue_block);
}

// TODO: breakpoints are gone, remove this.
pub type BreakpointHandler =
    Box<dyn Fn(BreakpointInfo) -> Result<(), Box<dyn Any + Send>> + Send + Sync + 'static>;

/// Information for a breakpoint
pub struct BreakpointInfo {
    /// Fault.
    pub fault: Option<()>,
}

// This is only called by C++ code, the 'pub' + '#[no_mangle]' combination
// prevents unused function elimination.
#[no_mangle]
pub unsafe extern "C" fn callback_trampoline(
    b: *mut Option<Box<dyn std::any::Any>>,
    callback: *mut BreakpointHandler,
) {
    let callback = Box::from_raw(callback);
    let result: Result<(), Box<dyn std::any::Any + Send>> =
        callback(BreakpointInfo { fault: None });
    match result {
        Ok(()) => *b = None,
        Err(e) => *b = Some(e),
    }
}

pub struct LLVMFunctionCodeGenerator<'ctx, 'a> {
    context: &'ctx Context,
    builder: Builder<'ctx>,
    intrinsics: &'a Intrinsics<'ctx>,
    state: State<'ctx>,
    function: FunctionValue<'ctx>,
    locals: Vec<PointerValue<'ctx>>, // Contains params and locals
    ctx: CtxType<'ctx, 'a>,
    unreachable_depth: usize,
    memory_plans: &'a PrimaryMap<MemoryIndex, MemoryPlan>,
    table_plans: &'a PrimaryMap<TableIndex, TablePlan>,

    // This is support for stackmaps:
    /*
    stackmaps: Rc<RefCell<StackmapRegistry>>,
    index: usize,
    opcode_offset: usize,
    track_state: bool,
    */
    module: &'a Module<'ctx>,
    vmoffsets: VMOffsets,
    wasm_module: &'a ModuleInfo,
    func_names: &'a SecondaryMap<FunctionIndex, String>,
}

impl<'ctx, 'a> LLVMFunctionCodeGenerator<'ctx, 'a> {
    fn translate_operator(
        &mut self,
        op: Operator,
        module: &ModuleInfo,
        _source_loc: u32,
    ) -> Result<(), CompileError> {
        // TODO: remove this vmctx by moving everything into CtxType. Values
        // computed off vmctx usually benefit from caching.
        let vmctx = &self.ctx.basic().into_pointer_value();

        //let opcode_offset: Option<usize> = None;

        if !self.state.reachable {
            match op {
                Operator::Block { ty: _ } | Operator::Loop { ty: _ } | Operator::If { ty: _ } => {
                    self.unreachable_depth += 1;
                    return Ok(());
                }
                Operator::Else => {
                    if self.unreachable_depth != 0 {
                        return Ok(());
                    }
                }
                Operator::End => {
                    if self.unreachable_depth != 0 {
                        self.unreachable_depth -= 1;
                        return Ok(());
                    }
                }
                _ => {
                    return Ok(());
                }
            }
        }

        match op {
            /***************************
             * Control Flow instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#control-flow-instructions
             ***************************/
            Operator::Block { ty } => {
                let current_block =
                    self.builder
                        .get_insert_block()
                        .ok_or(CompileError::Codegen(
                            "not currently in a block".to_string(),
                        ))?;

                let end_block = self.context.append_basic_block(self.function, "end");
                self.builder.position_at_end(end_block);

                let phis = if let Ok(wasmer_ty) = blocktype_to_type(ty) {
                    let llvm_ty = type_to_llvm(self.intrinsics, wasmer_ty);
                    [llvm_ty]
                        .iter()
                        .map(|&ty| self.builder.build_phi(ty, ""))
                        .collect()
                } else {
                    SmallVec::new()
                };

                self.state.push_block(end_block, phis);
                self.builder.position_at_end(current_block);
            }
            Operator::Loop { ty } => {
                let loop_body = self.context.append_basic_block(self.function, "loop_body");
                let loop_next = self.context.append_basic_block(self.function, "loop_outer");

                self.builder.build_unconditional_branch(loop_body);

                self.builder.position_at_end(loop_next);
                let phis = if let Ok(wasmer_ty) = blocktype_to_type(ty) {
                    let llvm_ty = type_to_llvm(self.intrinsics, wasmer_ty);
                    [llvm_ty]
                        .iter()
                        .map(|&ty| self.builder.build_phi(ty, ""))
                        .collect()
                } else {
                    SmallVec::new()
                };

                self.builder.position_at_end(loop_body);

                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        emit_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Loop,
                            &self.self.locals,
                            state,
                            ctx,
                            offset,
                        );
                        let signal_mem = ctx.signal_mem();
                        let iv = self.builder
                            .build_store(signal_mem, self.context.i8_type().const_int(0 as u64, false));
                        // Any 'store' can be made volatile.
                        iv.set_volatile(true).unwrap();
                        finalize_opcode_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Loop,
                            offset,
                        );
                    }
                }
                */

                self.state.push_loop(loop_body, loop_next, phis);
            }
            Operator::Br { relative_depth } => {
                let frame = self.state.frame_at_depth(relative_depth)?;

                let current_block =
                    self.builder
                        .get_insert_block()
                        .ok_or(CompileError::Codegen(
                            "not currently in a block".to_string(),
                        ))?;

                let value_len = if frame.is_loop() {
                    0
                } else {
                    frame.phis().len()
                };

                let values = self.state.peekn_extra(value_len)?;
                let values = values.iter().map(|(v, info)| {
                    apply_pending_canonicalization(&self.builder, self.intrinsics, *v, *info)
                });

                // For each result of the block we're branching to,
                // pop a value off the value stack and load it into
                // the corresponding phi.
                for (phi, value) in frame.phis().iter().zip(values) {
                    phi.add_incoming(&[(&value, current_block)]);
                }

                self.builder.build_unconditional_branch(*frame.br_dest());

                self.state.popn(value_len)?;
                self.state.reachable = false;
            }
            Operator::BrIf { relative_depth } => {
                let cond = self.state.pop1()?;
                let frame = self.state.frame_at_depth(relative_depth)?;

                let current_block =
                    self.builder
                        .get_insert_block()
                        .ok_or(CompileError::Codegen(
                            "not currently in a block".to_string(),
                        ))?;

                let value_len = if frame.is_loop() {
                    0
                } else {
                    frame.phis().len()
                };

                let param_stack = self.state.peekn_extra(value_len)?;
                let param_stack = param_stack.iter().map(|(v, info)| {
                    apply_pending_canonicalization(&self.builder, self.intrinsics, *v, *info)
                });

                for (phi, value) in frame.phis().iter().zip(param_stack) {
                    phi.add_incoming(&[(&value, current_block)]);
                }

                let else_block = self.context.append_basic_block(self.function, "else");

                let cond_value = self.builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    self.intrinsics.i32_zero,
                    "",
                );
                self.builder
                    .build_conditional_branch(cond_value, *frame.br_dest(), else_block);
                self.builder.position_at_end(else_block);
            }
            Operator::BrTable { ref table } => {
                let current_block =
                    self.builder
                        .get_insert_block()
                        .ok_or(CompileError::Codegen(
                            "not currently in a block".to_string(),
                        ))?;

                let (label_depths, default_depth) = table.read_table().map_err(to_wasm_error)?;

                let index = self.state.pop1()?;

                let default_frame = self.state.frame_at_depth(default_depth)?;

                let args = if default_frame.is_loop() {
                    Vec::new()
                } else {
                    let res_len = default_frame.phis().len();
                    self.state.peekn(res_len)?
                };

                for (phi, value) in default_frame.phis().iter().zip(args.iter()) {
                    phi.add_incoming(&[(value, current_block)]);
                }

                let cases: Vec<_> = label_depths
                    .iter()
                    .enumerate()
                    .map(|(case_index, &depth)| {
                        let frame_result: Result<&ControlFrame, CompileError> =
                            self.state.frame_at_depth(depth);
                        let frame = match frame_result {
                            Ok(v) => v,
                            Err(e) => return Err(e),
                        };
                        let case_index_literal =
                            self.context.i32_type().const_int(case_index as u64, false);

                        for (phi, value) in frame.phis().iter().zip(args.iter()) {
                            phi.add_incoming(&[(value, current_block)]);
                        }

                        Ok((case_index_literal, *frame.br_dest()))
                    })
                    .collect::<Result<_, _>>()?;

                self.builder.build_switch(
                    index.into_int_value(),
                    *default_frame.br_dest(),
                    &cases[..],
                );

                let args_len = args.len();
                self.state.popn(args_len)?;
                self.state.reachable = false;
            }
            Operator::If { ty } => {
                let current_block =
                    self.builder
                        .get_insert_block()
                        .ok_or(CompileError::Codegen(
                            "not currently in a block".to_string(),
                        ))?;
                let if_then_block = self.context.append_basic_block(self.function, "if_then");
                let if_else_block = self.context.append_basic_block(self.function, "if_else");
                let end_block = self.context.append_basic_block(self.function, "if_end");

                let end_phis = {
                    self.builder.position_at_end(end_block);

                    let phis = if let Ok(wasmer_ty) = blocktype_to_type(ty) {
                        let llvm_ty = type_to_llvm(self.intrinsics, wasmer_ty);
                        [llvm_ty]
                            .iter()
                            .map(|&ty| self.builder.build_phi(ty, ""))
                            .collect()
                    } else {
                        SmallVec::new()
                    };

                    self.builder.position_at_end(current_block);
                    phis
                };

                let cond = self.state.pop1()?;

                let cond_value = self.builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    self.intrinsics.i32_zero,
                    "",
                );

                self.builder
                    .build_conditional_branch(cond_value, if_then_block, if_else_block);
                self.builder.position_at_end(if_then_block);
                self.state
                    .push_if(if_then_block, if_else_block, end_block, end_phis);
            }
            Operator::Else => {
                if self.state.reachable {
                    let frame = self.state.frame_at_depth(0)?;
                    let current_block =
                        self.builder
                            .get_insert_block()
                            .ok_or(CompileError::Codegen(
                                "not currently in a block".to_string(),
                            ))?;

                    for phi in frame.phis().to_vec().iter().rev() {
                        let (value, info) = self.state.pop1_extra()?;
                        let value = apply_pending_canonicalization(
                            &self.builder,
                            self.intrinsics,
                            value,
                            info,
                        );
                        phi.add_incoming(&[(&value, current_block)])
                    }
                    let frame = self.state.frame_at_depth(0)?;
                    self.builder.build_unconditional_branch(*frame.code_after());
                }

                let (if_else_block, if_else_state) = if let ControlFrame::IfElse {
                    if_else,
                    if_else_state,
                    ..
                } = self.state.frame_at_depth_mut(0)?
                {
                    (if_else, if_else_state)
                } else {
                    unreachable!()
                };

                *if_else_state = IfElseState::Else;

                self.builder.position_at_end(*if_else_block);
                self.state.reachable = true;
            }

            Operator::End => {
                let frame = self.state.pop_frame()?;
                let current_block =
                    self.builder
                        .get_insert_block()
                        .ok_or(CompileError::Codegen(
                            "not currently in a block".to_string(),
                        ))?;

                if self.state.reachable {
                    for phi in frame.phis().iter().rev() {
                        let (value, info) = self.state.pop1_extra()?;
                        let value = apply_pending_canonicalization(
                            &self.builder,
                            self.intrinsics,
                            value,
                            info,
                        );
                        phi.add_incoming(&[(&value, current_block)]);
                    }

                    self.builder.build_unconditional_branch(*frame.code_after());
                }

                if let ControlFrame::IfElse {
                    if_else,
                    next,
                    if_else_state,
                    ..
                } = &frame
                {
                    if let IfElseState::If = if_else_state {
                        self.builder.position_at_end(*if_else);
                        self.builder.build_unconditional_branch(*next);
                    }
                }

                self.builder.position_at_end(*frame.code_after());
                self.state.reset_stack(&frame);

                self.state.reachable = true;

                // Push each phi value to the value stack.
                for phi in frame.phis() {
                    if phi.count_incoming() != 0 {
                        self.state.push1(phi.as_basic_value());
                    } else {
                        let basic_ty = phi.as_basic_value().get_type();
                        let placeholder_value = match basic_ty {
                            BasicTypeEnum::IntType(int_ty) => {
                                int_ty.const_int(0, false).as_basic_value_enum()
                            }
                            BasicTypeEnum::FloatType(float_ty) => {
                                float_ty.const_float(0.0).as_basic_value_enum()
                            }
                            _ => {
                                return Err(CompileError::Codegen(
                                    "Operator::End phi type unimplemented".to_string(),
                                ));
                            }
                        };
                        self.state.push1(placeholder_value);
                        phi.as_instruction().erase_from_basic_block();
                    }
                }
            }
            Operator::Return => {
                let current_block =
                    self.builder
                        .get_insert_block()
                        .ok_or(CompileError::Codegen(
                            "not currently in a block".to_string(),
                        ))?;

                let frame = self.state.outermost_frame()?;
                for phi in frame.phis().to_vec().iter() {
                    let (arg, info) = self.state.pop1_extra()?;
                    let arg =
                        apply_pending_canonicalization(&self.builder, self.intrinsics, arg, info);
                    phi.add_incoming(&[(&arg, current_block)]);
                }

                let frame = self.state.outermost_frame()?;
                self.builder.build_unconditional_branch(*frame.br_dest());

                self.state.reachable = false;
            }

            Operator::Unreachable => {
                // Emit an unreachable instruction.
                // If llvm cannot prove that this is never reached,
                // it will emit a `ud2` instruction on x86_64 arches.

                // Comment out this `if` block to allow spectests to pass.
                // TODO: fix this
                /*
                if let Some(offset) = opcode_offset {
                    if self.track_state {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        emit_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Trappable,
                            &self.self.locals,
                            state,
                            ctx,
                            offset,
                        );
                        self.builder.build_call(self.intrinsics.trap, &[], "trap");
                        finalize_opcode_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Trappable,
                            offset,
                        );
                    }
                }
                */

                self.builder.build_call(
                    self.intrinsics.throw_trap,
                    &[self.intrinsics.trap_unreachable],
                    "throw",
                );
                self.builder.build_unreachable();

                self.state.reachable = false;
            }

            /***************************
             * Basic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#basic-instructions
             ***************************/
            Operator::Nop => {
                // Do nothing.
            }
            Operator::Drop => {
                self.state.pop1()?;
            }

            // Generate const values.
            Operator::I32Const { value } => {
                let i = self.intrinsics.i32_ty.const_int(value as u64, false);
                let info = if is_f32_arithmetic(value as u32) {
                    ExtraInfo::arithmetic_f32()
                } else {
                    Default::default()
                };
                self.state.push1_extra(i, info);
            }
            Operator::I64Const { value } => {
                let i = self.intrinsics.i64_ty.const_int(value as u64, false);
                let info = if is_f64_arithmetic(value as u64) {
                    ExtraInfo::arithmetic_f64()
                } else {
                    Default::default()
                };
                self.state.push1_extra(i, info);
            }
            Operator::F32Const { value } => {
                let bits = self.intrinsics.i32_ty.const_int(value.bits() as u64, false);
                let info = if is_f32_arithmetic(value.bits()) {
                    ExtraInfo::arithmetic_f32()
                } else {
                    Default::default()
                };
                let f = self
                    .builder
                    .build_bitcast(bits, self.intrinsics.f32_ty, "f");
                self.state.push1_extra(f, info);
            }
            Operator::F64Const { value } => {
                let bits = self.intrinsics.i64_ty.const_int(value.bits(), false);
                let info = if is_f64_arithmetic(value.bits()) {
                    ExtraInfo::arithmetic_f64()
                } else {
                    Default::default()
                };
                let f = self
                    .builder
                    .build_bitcast(bits, self.intrinsics.f64_ty, "f");
                self.state.push1_extra(f, info);
            }
            Operator::V128Const { value } => {
                let mut hi: [u8; 8] = Default::default();
                let mut lo: [u8; 8] = Default::default();
                hi.copy_from_slice(&value.bytes()[0..8]);
                lo.copy_from_slice(&value.bytes()[8..16]);
                let packed = [u64::from_le_bytes(hi), u64::from_le_bytes(lo)];
                let i = self
                    .intrinsics
                    .i128_ty
                    .const_int_arbitrary_precision(&packed);
                let mut quad1: [u8; 4] = Default::default();
                let mut quad2: [u8; 4] = Default::default();
                let mut quad3: [u8; 4] = Default::default();
                let mut quad4: [u8; 4] = Default::default();
                quad1.copy_from_slice(&value.bytes()[0..4]);
                quad2.copy_from_slice(&value.bytes()[4..8]);
                quad3.copy_from_slice(&value.bytes()[8..12]);
                quad4.copy_from_slice(&value.bytes()[12..16]);
                let mut info: ExtraInfo = Default::default();
                if is_f32_arithmetic(u32::from_le_bytes(quad1))
                    && is_f32_arithmetic(u32::from_le_bytes(quad2))
                    && is_f32_arithmetic(u32::from_le_bytes(quad3))
                    && is_f32_arithmetic(u32::from_le_bytes(quad4))
                {
                    info |= ExtraInfo::arithmetic_f32();
                }
                if is_f64_arithmetic(packed[0]) && is_f64_arithmetic(packed[1]) {
                    info |= ExtraInfo::arithmetic_f64();
                }
                self.state.push1_extra(i, info);
            }

            Operator::I8x16Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let v = v.into_int_value();
                let v = self
                    .builder
                    .build_int_truncate(v, self.intrinsics.i8_ty, "");
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v.as_basic_value_enum(),
                    self.intrinsics.i8x16_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(res, i);
            }
            Operator::I16x8Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let v = v.into_int_value();
                let v = self
                    .builder
                    .build_int_truncate(v, self.intrinsics.i16_ty, "");
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v.as_basic_value_enum(),
                    self.intrinsics.i16x8_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(res, i);
            }
            Operator::I32x4Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v,
                    self.intrinsics.i32x4_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(res, i);
            }
            Operator::I64x2Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v,
                    self.intrinsics.i64x2_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(res, i);
            }
            Operator::F32x4Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v,
                    self.intrinsics.f32x4_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // The spec is unclear, we interpret splat as preserving NaN
                // payload bits.
                self.state.push1_extra(res, i);
            }
            Operator::F64x2Splat => {
                let (v, i) = self.state.pop1_extra()?;
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v,
                    self.intrinsics.f64x2_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // The spec is unclear, we interpret splat as preserving NaN
                // payload bits.
                self.state.push1_extra(res, i);
            }

            // Operate on self.locals.
            Operator::LocalGet { local_index } => {
                let pointer_value = self.locals[local_index as usize];
                let v = self.builder.build_load(pointer_value, "");
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    format!("local {}", local_index),
                    v.as_instruction_value().unwrap(),
                );
                self.state.push1(v);
            }
            Operator::LocalSet { local_index } => {
                let pointer_value = self.locals[local_index as usize];
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let store = self.builder.build_store(pointer_value, v);
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    format!("local {}", local_index),
                    store,
                );
            }
            Operator::LocalTee { local_index } => {
                let pointer_value = self.locals[local_index as usize];
                let (v, i) = self.state.peek1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let store = self.builder.build_store(pointer_value, v);
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    format!("local {}", local_index),
                    store,
                );
            }

            Operator::GlobalGet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);
                match self.ctx.global(global_index, self.intrinsics) {
                    GlobalCache::Const { value } => {
                        self.state.push1(value);
                    }
                    GlobalCache::Mut { ptr_to_value } => {
                        let value = self.builder.build_load(ptr_to_value, "");
                        // TODO: tbaa
                        self.state.push1(value);
                    }
                }
            }
            Operator::GlobalSet { global_index } => {
                let global_index = GlobalIndex::from_u32(global_index);
                match self.ctx.global(global_index, self.intrinsics) {
                    GlobalCache::Const { value } => {
                        return Err(CompileError::Codegen(format!(
                            "global.set on immutable global index {}",
                            global_index.as_u32()
                        )))
                    }
                    GlobalCache::Mut { ptr_to_value } => {
                        let (value, info) = self.state.pop1_extra()?;
                        let value = apply_pending_canonicalization(
                            &self.builder,
                            self.intrinsics,
                            value,
                            info,
                        );
                        self.builder.build_store(ptr_to_value, value);
                        // TODO: tbaa
                    }
                }
            }

            Operator::Select => {
                let ((v1, i1), (v2, i2), (cond, _)) = self.state.pop3_extra()?;
                // We don't bother canonicalizing 'cond' here because we only
                // compare it to zero, and that's invariant under
                // canonicalization.

                // If the pending bits of v1 and v2 are the same, we can pass
                // them along to the result. Otherwise, apply pending
                // canonicalizations now.
                let (v1, i1, v2, i2) = if i1.has_pending_f32_nan() != i2.has_pending_f32_nan()
                    || i1.has_pending_f64_nan() != i2.has_pending_f64_nan()
                {
                    (
                        apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1),
                        i1.strip_pending(),
                        apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2),
                        i2.strip_pending(),
                    )
                } else {
                    (v1, i1, v2, i2)
                };
                let cond_value = self.builder.build_int_compare(
                    IntPredicate::NE,
                    cond.into_int_value(),
                    self.intrinsics.i32_zero,
                    "",
                );
                let res = self.builder.build_select(cond_value, v1, v2, "");
                let info = {
                    let mut info = i1.strip_pending() & i2.strip_pending();
                    if i1.has_pending_f32_nan() {
                        debug_assert!(i2.has_pending_f32_nan());
                        info |= ExtraInfo::pending_f32_nan();
                    }
                    if i1.has_pending_f64_nan() {
                        debug_assert!(i2.has_pending_f64_nan());
                        info |= ExtraInfo::pending_f64_nan();
                    }
                    info
                };
                self.state.push1_extra(res, info);
            }
            Operator::Call { function_index } => {
                let func_index = FunctionIndex::from_u32(function_index);
                let sigindex = &module.functions[func_index];
                let func_type = &module.signatures[*sigindex];
                let func_name = &self.func_names[func_index];
                let llvm_func_type = func_type_to_llvm(&self.context, &self.intrinsics, func_type);

                let (func, callee_vmctx) = if let Some(local_func_index) =
                    module.local_func_index(func_index)
                {
                    // TODO: we could do this by comparing self.function indices instead
                    // of going through LLVM APIs and string comparisons.
                    let func = self.module.get_function(func_name);
                    let func = if func.is_none() {
                        self.module
                            .add_function(func_name, llvm_func_type, Some(Linkage::External))
                    } else {
                        func.unwrap()
                    };
                    (func.as_global_value().as_pointer_value(), self.ctx.basic())
                } else {
                    let offset = self.vmoffsets.vmctx_vmfunction_import(func_index);
                    let offset = self.intrinsics.i32_ty.const_int(offset.into(), false);
                    let vmfunction_import_ptr =
                        unsafe { self.builder.build_gep(*vmctx, &[offset], "") };
                    let vmfunction_import_ptr = self
                        .builder
                        .build_bitcast(
                            vmfunction_import_ptr,
                            self.intrinsics.vmfunction_import_ptr_ty,
                            "",
                        )
                        .into_pointer_value();

                    let body_ptr_ptr = self
                        .builder
                        .build_struct_gep(
                            vmfunction_import_ptr,
                            self.intrinsics.vmfunction_import_body_element,
                            "",
                        )
                        .unwrap();
                    let body_ptr = self.builder.build_load(body_ptr_ptr, "");
                    let body_ptr = self
                        .builder
                        .build_bitcast(body_ptr, llvm_func_type.ptr_type(AddressSpace::Generic), "")
                        .into_pointer_value();
                    let vmctx_ptr_ptr = self
                        .builder
                        .build_struct_gep(
                            vmfunction_import_ptr,
                            self.intrinsics.vmfunction_import_vmctx_element,
                            "",
                        )
                        .unwrap();
                    let vmctx_ptr = self.builder.build_load(vmctx_ptr_ptr, "");
                    (body_ptr, vmctx_ptr)
                };

                let params: Vec<_> = std::iter::once(callee_vmctx)
                    .chain(
                        self.state
                            .peekn_extra(func_type.params().len())?
                            .iter()
                            .enumerate()
                            .map(|(i, (v, info))| match func_type.params()[i] {
                                Type::F32 => self.builder.build_bitcast(
                                    apply_pending_canonicalization(
                                        &self.builder,
                                        self.intrinsics,
                                        *v,
                                        *info,
                                    ),
                                    self.intrinsics.f32_ty,
                                    "",
                                ),
                                Type::F64 => self.builder.build_bitcast(
                                    apply_pending_canonicalization(
                                        &self.builder,
                                        self.intrinsics,
                                        *v,
                                        *info,
                                    ),
                                    self.intrinsics.f64_ty,
                                    "",
                                ),
                                Type::V128 => apply_pending_canonicalization(
                                    &self.builder,
                                    self.intrinsics,
                                    *v,
                                    *info,
                                ),
                                _ => *v,
                            }),
                    )
                    .collect();

                /*
                let func_ptr = self.llvm.functions.borrow_mut()[&func_index];

                (params, func_ptr.as_global_value().as_pointer_value())
                */
                self.state.popn(func_type.params().len())?;
                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        emit_stack_map(
                            &info,
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Call,
                            &self.locals,
                            state,
                            ctx,
                            offset,
                        )
                    }
                }
                */
                let call_site = self.builder.build_call(func, &params, "");
                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        finalize_opcode_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Call,
                            offset,
                        )
                    }
                }
                */

                if let Some(basic_value) = call_site.try_as_basic_value().left() {
                    match func_type.results().len() {
                        1 => self.state.push1(basic_value),
                        count @ _ => {
                            // This is a multi-value return.
                            let struct_value = basic_value.into_struct_value();
                            for i in 0..(count as u32) {
                                let value = self
                                    .builder
                                    .build_extract_value(struct_value, i, "")
                                    .unwrap();
                                self.state.push1(value);
                            }
                        }
                    }
                }
            }
            Operator::CallIndirect { index, table_index } => {
                let sigindex = SignatureIndex::from_u32(index);
                let func_type = &module.signatures[sigindex];
                let expected_dynamic_sigindex =
                    self.ctx.dynamic_sigindex(sigindex, self.intrinsics);
                let (table_base, table_bound) = self.ctx.table(
                    TableIndex::from_u32(table_index),
                    self.intrinsics,
                    self.module,
                    &self.builder,
                );
                let func_index = self.state.pop1()?.into_int_value();

                // We assume the table has the `anyfunc` element type.
                let casted_table_base = self.builder.build_pointer_cast(
                    table_base,
                    self.intrinsics.anyfunc_ty.ptr_type(AddressSpace::Generic),
                    "casted_table_base",
                );

                let anyfunc_struct_ptr = unsafe {
                    self.builder.build_in_bounds_gep(
                        casted_table_base,
                        &[func_index],
                        "anyfunc_struct_ptr",
                    )
                };

                // Load things from the anyfunc data structure.
                let (func_ptr, found_dynamic_sigindex, ctx_ptr) = unsafe {
                    (
                        self.builder
                            .build_load(
                                self.builder
                                    .build_struct_gep(anyfunc_struct_ptr, 0, "func_ptr_ptr")
                                    .unwrap(),
                                "func_ptr",
                            )
                            .into_pointer_value(),
                        self.builder
                            .build_load(
                                self.builder
                                    .build_struct_gep(anyfunc_struct_ptr, 1, "sigindex_ptr")
                                    .unwrap(),
                                "sigindex",
                            )
                            .into_int_value(),
                        self.builder.build_load(
                            self.builder
                                .build_struct_gep(anyfunc_struct_ptr, 2, "ctx_ptr_ptr")
                                .unwrap(),
                            "ctx_ptr",
                        ),
                    )
                };

                let truncated_table_bounds = self.builder.build_int_truncate(
                    table_bound,
                    self.intrinsics.i32_ty,
                    "truncated_table_bounds",
                );

                // First, check if the index is outside of the table bounds.
                let index_in_bounds = self.builder.build_int_compare(
                    IntPredicate::ULT,
                    func_index,
                    truncated_table_bounds,
                    "index_in_bounds",
                );

                let index_in_bounds = self
                    .builder
                    .build_call(
                        self.intrinsics.expect_i1,
                        &[
                            index_in_bounds.as_basic_value_enum(),
                            self.intrinsics
                                .i1_ty
                                .const_int(1, false)
                                .as_basic_value_enum(),
                        ],
                        "index_in_bounds_expect",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();

                let in_bounds_continue_block = self
                    .context
                    .append_basic_block(self.function, "in_bounds_continue_block");
                let not_in_bounds_block = self
                    .context
                    .append_basic_block(self.function, "not_in_bounds_block");
                self.builder.build_conditional_branch(
                    index_in_bounds,
                    in_bounds_continue_block,
                    not_in_bounds_block,
                );
                self.builder.position_at_end(not_in_bounds_block);
                self.builder.build_call(
                    self.intrinsics.throw_trap,
                    &[self.intrinsics.trap_table_access_oob],
                    "throw",
                );
                self.builder.build_unreachable();
                self.builder.position_at_end(in_bounds_continue_block);

                // Next, check if the table element is initialized.

                let elem_initialized = self.builder.build_is_not_null(func_ptr, "");

                // Next, check if the signature id is correct.

                let sigindices_equal = self.builder.build_int_compare(
                    IntPredicate::EQ,
                    expected_dynamic_sigindex,
                    found_dynamic_sigindex,
                    "sigindices_equal",
                );

                let initialized_and_sigindices_match =
                    self.builder
                        .build_and(elem_initialized, sigindices_equal, "");

                // Tell llvm that `expected_dynamic_sigindex` should equal `found_dynamic_sigindex`.
                let initialized_and_sigindices_match = self
                    .builder
                    .build_call(
                        self.intrinsics.expect_i1,
                        &[
                            initialized_and_sigindices_match.as_basic_value_enum(),
                            self.intrinsics
                                .i1_ty
                                .const_int(1, false)
                                .as_basic_value_enum(),
                        ],
                        "initialized_and_sigindices_match_expect",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();

                let continue_block = self
                    .context
                    .append_basic_block(self.function, "continue_block");
                let sigindices_notequal_block = self
                    .context
                    .append_basic_block(self.function, "sigindices_notequal_block");
                self.builder.build_conditional_branch(
                    initialized_and_sigindices_match,
                    continue_block,
                    sigindices_notequal_block,
                );

                self.builder.position_at_end(sigindices_notequal_block);
                let trap_code = self.builder.build_select(
                    elem_initialized,
                    self.intrinsics.trap_call_indirect_sig,
                    self.intrinsics.trap_call_indirect_null,
                    "",
                );
                self.builder
                    .build_call(self.intrinsics.throw_trap, &[trap_code], "throw");
                self.builder.build_unreachable();
                self.builder.position_at_end(continue_block);

                let llvm_func_type = func_type_to_llvm(&self.context, &self.intrinsics, func_type);

                let pushed_args = self.state.popn_save_extra(func_type.params().len())?;

                let args: Vec<_> = std::iter::once(ctx_ptr)
                    .chain(pushed_args.into_iter().enumerate().map(|(i, (v, info))| {
                        match func_type.params()[i] {
                            Type::F32 => self.builder.build_bitcast(
                                apply_pending_canonicalization(
                                    &self.builder,
                                    self.intrinsics,
                                    v,
                                    info,
                                ),
                                self.intrinsics.f32_ty,
                                "",
                            ),
                            Type::F64 => self.builder.build_bitcast(
                                apply_pending_canonicalization(
                                    &self.builder,
                                    self.intrinsics,
                                    v,
                                    info,
                                ),
                                self.intrinsics.f64_ty,
                                "",
                            ),
                            Type::V128 => apply_pending_canonicalization(
                                &self.builder,
                                self.intrinsics,
                                v,
                                info,
                            ),
                            _ => v,
                        }
                    }))
                    .collect();

                let typed_func_ptr = self.builder.build_pointer_cast(
                    func_ptr,
                    llvm_func_type.ptr_type(AddressSpace::Generic),
                    "typed_func_ptr",
                );

                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        emit_stack_map(
                            &info,
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Call,
                            &self.locals,
                            state,
                            ctx,
                            offset,
                        )
                    }
                }
                */
                let call_site = self
                    .builder
                    .build_call(typed_func_ptr, &args, "indirect_call");
                /*
                if self.track_state {
                    if let Some(offset) = opcode_offset {
                        let mut stackmaps = self.stackmaps.borrow_mut();
                        finalize_opcode_stack_map(
                            self.intrinsics,
                            self.builder,
                            self.index,
                            &mut *stackmaps,
                            StackmapEntryKind::Call,
                            offset,
                        )
                    }
                }
                */

                match func_type.results() {
                    [] => {}
                    [_] => {
                        let value = call_site.try_as_basic_value().left().unwrap();
                        self.state.push1(match func_type.results()[0] {
                            Type::F32 => self.builder.build_bitcast(
                                value,
                                self.intrinsics.f32_ty,
                                "ret_cast",
                            ),
                            Type::F64 => self.builder.build_bitcast(
                                value,
                                self.intrinsics.f64_ty,
                                "ret_cast",
                            ),
                            _ => value,
                        });
                    }
                    _ => {
                        return Err(CompileError::Codegen(
                            "Operator::CallIndirect multi-value returns unimplemented".to_string(),
                        ));
                    }
                }
            }

            /***************************
             * Integer Arithmetic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#integer-arithmetic-instructions
             ***************************/
            Operator::I32Add | Operator::I64Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = self.builder.build_int_add(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I8x16Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_add(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_add(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_add(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I64x2Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_add(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I8x16AddSaturateS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.sadd_sat_i8x16,
                        &[v1.as_basic_value_enum(), v2.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8AddSaturateS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.sadd_sat_i16x8,
                        &[v1.as_basic_value_enum(), v2.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I8x16AddSaturateU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.uadd_sat_i8x16,
                        &[v1.as_basic_value_enum(), v2.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8AddSaturateU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.uadd_sat_i16x8,
                        &[v1.as_basic_value_enum(), v2.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32Sub | Operator::I64Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = self.builder.build_int_sub(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I8x16Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_sub(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_sub(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_sub(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I64x2Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_sub(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I8x16SubSaturateS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.ssub_sat_i8x16,
                        &[v1.as_basic_value_enum(), v2.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8SubSaturateS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.ssub_sat_i16x8,
                        &[v1.as_basic_value_enum(), v2.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I8x16SubSaturateU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.usub_sat_i8x16,
                        &[v1.as_basic_value_enum(), v2.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8SubSaturateU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.usub_sat_i16x8,
                        &[v1.as_basic_value_enum(), v2.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32Mul | Operator::I64Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = self.builder.build_int_mul(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I8x16Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_mul(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_mul(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_mul(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32DivS | Operator::I64DivS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero_or_overflow(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    v1,
                    v2,
                );

                let res = self.builder.build_int_signed_div(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I32DivU | Operator::I64DivU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    v2,
                );

                let res = self.builder.build_int_unsigned_div(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I32RemS | Operator::I64RemS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let int_type = v1.get_type();
                let (min_value, neg_one_value) = if int_type == self.intrinsics.i32_ty {
                    let min_value = int_type.const_int(i32::min_value() as u64, false);
                    let neg_one_value = int_type.const_int(-1i32 as u32 as u64, false);
                    (min_value, neg_one_value)
                } else if int_type == self.intrinsics.i64_ty {
                    let min_value = int_type.const_int(i64::min_value() as u64, false);
                    let neg_one_value = int_type.const_int(-1i64 as u64, false);
                    (min_value, neg_one_value)
                } else {
                    unreachable!()
                };

                trap_if_zero(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    v2,
                );

                // "Overflow also leads to undefined behavior; this is a rare
                // case, but can occur, for example, by taking the remainder of
                // a 32-bit division of -2147483648 by -1. (The remainder
                // doesn’t actually overflow, but this rule lets srem be
                // implemented using instructions that return both the result
                // of the division and the remainder.)"
                //   -- https://llvm.org/docs/LangRef.html#srem-instruction
                //
                // In Wasm, the i32.rem_s i32.const -2147483648 i32.const -1 is
                // i32.const 0. We implement this by swapping out the left value
                // for 0 in this case.
                let will_overflow = self.builder.build_and(
                    self.builder
                        .build_int_compare(IntPredicate::EQ, v1, min_value, "left_is_min"),
                    self.builder.build_int_compare(
                        IntPredicate::EQ,
                        v2,
                        neg_one_value,
                        "right_is_neg_one",
                    ),
                    "srem_will_overflow",
                );
                let v1 = self
                    .builder
                    .build_select(will_overflow, int_type.const_zero(), v1, "")
                    .into_int_value();
                let res = self.builder.build_int_signed_rem(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I32RemU | Operator::I64RemU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());

                trap_if_zero(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    v2,
                );

                let res = self.builder.build_int_unsigned_rem(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I32And | Operator::I64And | Operator::V128And => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = self.builder.build_and(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I32Or | Operator::I64Or | Operator::V128Or => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = self.builder.build_or(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I32Xor | Operator::I64Xor | Operator::V128Xor => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = self.builder.build_xor(v1, v2, "");
                self.state.push1(res);
            }
            Operator::V128Bitselect => {
                let ((v1, i1), (v2, i2), (cond, cond_info)) = self.state.pop3_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let cond =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, cond, cond_info);
                let v1 = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i1x128_ty, "")
                    .into_vector_value();
                let v2 = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i1x128_ty, "")
                    .into_vector_value();
                let cond = self
                    .builder
                    .build_bitcast(cond, self.intrinsics.i1x128_ty, "")
                    .into_vector_value();
                let res = self.builder.build_select(cond, v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32Shl | Operator::I64Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                // TODO: missing 'and' of v2?
                let res = self.builder.build_left_shift(v1, v2, "");
                self.state.push1(res);
            }
            Operator::I8x16Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 = self
                    .builder
                    .build_and(v2, self.intrinsics.i32_ty.const_int(7, false), "");
                let v2 = self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i8_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i8x16_ty,
                    "",
                );
                let res = self.builder.build_left_shift(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(15, false), "");
                let v2 = self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i16_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i16x8_ty,
                    "",
                );
                let res = self.builder.build_left_shift(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(31, false), "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i32x4_ty,
                    "",
                );
                let res = self.builder.build_left_shift(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I64x2Shl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i64x2(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(63, false), "");
                let v2 = self
                    .builder
                    .build_int_z_extend(v2, self.intrinsics.i64_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i64x2_ty,
                    "",
                );
                let res = self.builder.build_left_shift(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32ShrS | Operator::I64ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                // TODO: check wasm spec, is this missing v2 mod LaneBits?
                let res = self.builder.build_right_shift(v1, v2, true, "");
                self.state.push1(res);
            }
            Operator::I8x16ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 = self
                    .builder
                    .build_and(v2, self.intrinsics.i32_ty.const_int(7, false), "");
                let v2 = self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i8_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i8x16_ty,
                    "",
                );
                let res = self.builder.build_right_shift(v1, v2, true, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(15, false), "");
                let v2 = self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i16_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i16x8_ty,
                    "",
                );
                let res = self.builder.build_right_shift(v1, v2, true, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(31, false), "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i32x4_ty,
                    "",
                );
                let res = self.builder.build_right_shift(v1, v2, true, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I64x2ShrS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i64x2(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(63, false), "");
                let v2 = self
                    .builder
                    .build_int_z_extend(v2, self.intrinsics.i64_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i64x2_ty,
                    "",
                );
                let res = self.builder.build_right_shift(v1, v2, true, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32ShrU | Operator::I64ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let res = self.builder.build_right_shift(v1, v2, false, "");
                self.state.push1(res);
            }
            Operator::I8x16ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 = self
                    .builder
                    .build_and(v2, self.intrinsics.i32_ty.const_int(7, false), "");
                let v2 = self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i8_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i8x16_ty,
                    "",
                );
                let res = self.builder.build_right_shift(v1, v2, false, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(15, false), "");
                let v2 = self
                    .builder
                    .build_int_truncate(v2, self.intrinsics.i16_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i16x8_ty,
                    "",
                );
                let res = self.builder.build_right_shift(v1, v2, false, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(31, false), "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i32x4_ty,
                    "",
                );
                let res = self.builder.build_right_shift(v1, v2, false, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I64x2ShrU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i64x2(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let v2 =
                    self.builder
                        .build_and(v2, self.intrinsics.i32_ty.const_int(63, false), "");
                let v2 = self
                    .builder
                    .build_int_z_extend(v2, self.intrinsics.i64_ty, "");
                let v2 = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    v2.as_basic_value_enum(),
                    self.intrinsics.i64x2_ty,
                    "",
                );
                let res = self.builder.build_right_shift(v1, v2, false, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32Rotl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = self.builder.build_left_shift(v1, v2, "");
                let rhs = {
                    let int_width = self.intrinsics.i32_ty.const_int(32 as u64, false);
                    let rhs = self.builder.build_int_sub(int_width, v2, "");
                    self.builder.build_right_shift(v1, rhs, false, "")
                };
                let res = self.builder.build_or(lhs, rhs, "");
                self.state.push1(res);
            }
            Operator::I64Rotl => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = self.builder.build_left_shift(v1, v2, "");
                let rhs = {
                    let int_width = self.intrinsics.i64_ty.const_int(64 as u64, false);
                    let rhs = self.builder.build_int_sub(int_width, v2, "");
                    self.builder.build_right_shift(v1, rhs, false, "")
                };
                let res = self.builder.build_or(lhs, rhs, "");
                self.state.push1(res);
            }
            Operator::I32Rotr => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = self.builder.build_right_shift(v1, v2, false, "");
                let rhs = {
                    let int_width = self.intrinsics.i32_ty.const_int(32 as u64, false);
                    let rhs = self.builder.build_int_sub(int_width, v2, "");
                    self.builder.build_left_shift(v1, rhs, "")
                };
                let res = self.builder.build_or(lhs, rhs, "");
                self.state.push1(res);
            }
            Operator::I64Rotr => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let lhs = self.builder.build_right_shift(v1, v2, false, "");
                let rhs = {
                    let int_width = self.intrinsics.i64_ty.const_int(64 as u64, false);
                    let rhs = self.builder.build_int_sub(int_width, v2, "");
                    self.builder.build_left_shift(v1, rhs, "")
                };
                let res = self.builder.build_or(lhs, rhs, "");
                self.state.push1(res);
            }
            Operator::I32Clz => {
                let (input, info) = self.state.pop1_extra()?;
                let input =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, input, info);
                let is_zero_undef = self.intrinsics.i1_zero.as_basic_value_enum();
                let res = self
                    .builder
                    .build_call(self.intrinsics.ctlz_i32, &[input, is_zero_undef], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Clz => {
                let (input, info) = self.state.pop1_extra()?;
                let input =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, input, info);
                let is_zero_undef = self.intrinsics.i1_zero.as_basic_value_enum();
                let res = self
                    .builder
                    .build_call(self.intrinsics.ctlz_i64, &[input, is_zero_undef], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I32Ctz => {
                let (input, info) = self.state.pop1_extra()?;
                let input =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, input, info);
                let is_zero_undef = self.intrinsics.i1_zero.as_basic_value_enum();
                let res = self
                    .builder
                    .build_call(self.intrinsics.cttz_i32, &[input, is_zero_undef], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Ctz => {
                let (input, info) = self.state.pop1_extra()?;
                let input =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, input, info);
                let is_zero_undef = self.intrinsics.i1_zero.as_basic_value_enum();
                let res = self
                    .builder
                    .build_call(self.intrinsics.cttz_i64, &[input, is_zero_undef], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I32Popcnt => {
                let (input, info) = self.state.pop1_extra()?;
                let input =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, input, info);
                let res = self
                    .builder
                    .build_call(self.intrinsics.ctpop_i32, &[input], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Popcnt => {
                let (input, info) = self.state.pop1_extra()?;
                let input =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, input, info);
                let res = self
                    .builder
                    .build_call(self.intrinsics.ctpop_i64, &[input], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I32Eqz => {
                let input = self.state.pop1()?.into_int_value();
                let cond = self.builder.build_int_compare(
                    IntPredicate::EQ,
                    input,
                    self.intrinsics.i32_zero,
                    "",
                );
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Eqz => {
                let input = self.state.pop1()?.into_int_value();
                let cond = self.builder.build_int_compare(
                    IntPredicate::EQ,
                    input,
                    self.intrinsics.i64_zero,
                    "",
                );
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }

            /***************************
             * Floating-Point Arithmetic instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#floating-point-arithmetic-instructions
             ***************************/
            Operator::F32Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = self.builder.build_float_add(v1, v2, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f32_nan(),
                );
            }
            Operator::F64Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = self.builder.build_float_add(v1, v2, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f64_nan(),
                );
            }
            Operator::F32x4Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, i2) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_float_add(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f32_nan(),
                );
            }
            Operator::F64x2Add => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, i2) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_float_add(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f64_nan(),
                );
            }
            Operator::F32Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = self.builder.build_float_sub(v1, v2, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f32_nan(),
                );
            }
            Operator::F64Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = self.builder.build_float_sub(v1, v2, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f64_nan(),
                );
            }
            Operator::F32x4Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, i2) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_float_sub(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f32_nan(),
                );
            }
            Operator::F64x2Sub => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, i2) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_float_sub(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f64_nan(),
                );
            }
            Operator::F32Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = self.builder.build_float_mul(v1, v2, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f32_nan(),
                );
            }
            Operator::F64Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = self.builder.build_float_mul(v1, v2, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f64_nan(),
                );
            }
            Operator::F32x4Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, i2) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_float_mul(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f32_nan(),
                );
            }
            Operator::F64x2Mul => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, i2) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_float_mul(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(
                    res,
                    (i1.strip_pending() & i2.strip_pending()) | ExtraInfo::pending_f64_nan(),
                );
            }
            Operator::F32Div => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = self.builder.build_float_div(v1, v2, "");
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            Operator::F64Div => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let res = self.builder.build_float_div(v1, v2, "");
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32x4Div => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_float_div(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            Operator::F64x2Div => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_float_div(v1, v2, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32Sqrt => {
                let input = self.state.pop1()?;
                let res = self
                    .builder
                    .build_call(self.intrinsics.sqrt_f32, &[input], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            Operator::F64Sqrt => {
                let input = self.state.pop1()?;
                let res = self
                    .builder
                    .build_call(self.intrinsics.sqrt_f64, &[input], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32x4Sqrt => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_f32x4(&self.builder, self.intrinsics, v, i);
                let res = self
                    .builder
                    .build_call(self.intrinsics.sqrt_f32x4, &[v.as_basic_value_enum()], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let bits = self
                    .builder
                    .build_bitcast(res, self.intrinsics.i128_ty, "bits");
                self.state.push1_extra(bits, ExtraInfo::pending_f32_nan());
            }
            Operator::F64x2Sqrt => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_f64x2(&self.builder, self.intrinsics, v, i);
                let res = self
                    .builder
                    .build_call(self.intrinsics.sqrt_f64x2, &[v.as_basic_value_enum()], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let bits = self
                    .builder
                    .build_bitcast(res, self.intrinsics.i128_ty, "bits");
                self.state.push1(bits);
            }
            Operator::F32Min => {
                // This implements the same logic as LLVM's @llvm.minimum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 8 and LLVM 9.
                let (v1, v2) = self.state.pop2()?;

                // To detect min(-0.0, 0.0), we check whether the integer
                // representations are equal. There's one other case where that
                // can happen: non-canonical NaNs. Here we unconditionally
                // canonicalize the NaNs.
                let v1 = canonicalize_nans(&self.builder, self.intrinsics, v1);
                let v2 = canonicalize_nans(&self.builder, self.intrinsics, v2);

                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let v1_is_nan = self.builder.build_float_compare(
                    FloatPredicate::UNO,
                    v1,
                    self.intrinsics.f32_zero,
                    "nan",
                );
                let v2_is_not_nan = self.builder.build_float_compare(
                    FloatPredicate::ORD,
                    v2,
                    self.intrinsics.f32_zero,
                    "notnan",
                );
                let v1_repr = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i32_ty, "")
                    .into_int_value();
                let v2_repr = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i32_ty, "")
                    .into_int_value();
                let repr_ne =
                    self.builder
                        .build_int_compare(IntPredicate::NE, v1_repr, v2_repr, "");
                let float_eq = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let min_cmp = self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let negative_zero = self.intrinsics.f32_ty.const_float(-0.0);
                let v2 = self
                    .builder
                    .build_select(
                        self.builder.build_and(
                            self.builder.build_and(float_eq, repr_ne, ""),
                            v2_is_not_nan,
                            "",
                        ),
                        negative_zero,
                        v2,
                        "",
                    )
                    .into_float_value();
                let res = self.builder.build_select(
                    self.builder.build_or(v1_is_nan, min_cmp, ""),
                    v1,
                    v2,
                    "",
                );
                // Because inputs were canonicalized, we always produce
                // canonical NaN outputs. No pending NaN cleanup.
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::F64Min => {
                // This implements the same logic as LLVM's @llvm.minimum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 8 and LLVM 9.
                let (v1, v2) = self.state.pop2()?;

                // To detect min(-0.0, 0.0), we check whether the integer
                // representations are equal. There's one other case where that
                // can happen: non-canonical NaNs. Here we unconditionally
                // canonicalize the NaNs.
                let v1 = canonicalize_nans(&self.builder, self.intrinsics, v1);
                let v2 = canonicalize_nans(&self.builder, self.intrinsics, v2);

                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let v1_is_nan = self.builder.build_float_compare(
                    FloatPredicate::UNO,
                    v1,
                    self.intrinsics.f64_zero,
                    "nan",
                );
                let v2_is_not_nan = self.builder.build_float_compare(
                    FloatPredicate::ORD,
                    v2,
                    self.intrinsics.f64_zero,
                    "notnan",
                );
                let v1_repr = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i64_ty, "")
                    .into_int_value();
                let v2_repr = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i64_ty, "")
                    .into_int_value();
                let repr_ne =
                    self.builder
                        .build_int_compare(IntPredicate::NE, v1_repr, v2_repr, "");
                let float_eq = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let min_cmp = self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let negative_zero = self.intrinsics.f64_ty.const_float(-0.0);
                let v2 = self
                    .builder
                    .build_select(
                        self.builder.build_and(
                            self.builder.build_and(float_eq, repr_ne, ""),
                            v2_is_not_nan,
                            "",
                        ),
                        negative_zero,
                        v2,
                        "",
                    )
                    .into_float_value();
                let res = self.builder.build_select(
                    self.builder.build_or(v1_is_nan, min_cmp, ""),
                    v1,
                    v2,
                    "",
                );
                // Because inputs were canonicalized, we always produce
                // canonical NaN outputs. No pending NaN cleanup.
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::F32x4Min => {
                // This implements the same logic as LLVM's @llvm.minimum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 8 and LLVM 9.
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);

                // To detect min(-0.0, 0.0), we check whether the integer
                // representations are equal. There's one other case where that
                // can happen: non-canonical NaNs. Here we unconditionally
                // canonicalize the NaNs. Note that this is a different
                // canonicalization from that which may be performed in the
                // v128_into_f32x4 self.function. That may canonicalize as F64x2 if
                // previous computations may have emitted F64x2 NaNs.
                let v1 =
                    canonicalize_nans(&self.builder, self.intrinsics, v1.as_basic_value_enum());
                let v2 =
                    canonicalize_nans(&self.builder, self.intrinsics, v2.as_basic_value_enum());
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());

                let v1_is_nan = self.builder.build_float_compare(
                    FloatPredicate::UNO,
                    v1,
                    self.intrinsics.f32x4_zero,
                    "nan",
                );
                let v2_is_not_nan = self.builder.build_float_compare(
                    FloatPredicate::ORD,
                    v2,
                    self.intrinsics.f32x4_zero,
                    "notnan",
                );
                let v1_repr = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2_repr = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let repr_ne =
                    self.builder
                        .build_int_compare(IntPredicate::NE, v1_repr, v2_repr, "");
                let float_eq = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let min_cmp = self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let negative_zero = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    self.intrinsics
                        .f32_ty
                        .const_float(-0.0)
                        .as_basic_value_enum(),
                    self.intrinsics.f32x4_ty,
                    "",
                );
                let v2 = self
                    .builder
                    .build_select(
                        self.builder.build_and(
                            self.builder.build_and(float_eq, repr_ne, ""),
                            v2_is_not_nan,
                            "",
                        ),
                        negative_zero,
                        v2,
                        "",
                    )
                    .into_vector_value();
                let res = self.builder.build_select(
                    self.builder.build_or(v1_is_nan, min_cmp, ""),
                    v1,
                    v2,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // Because inputs were canonicalized, we always produce
                // canonical NaN outputs. No pending NaN cleanup.
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::F64x2Min => {
                // This implements the same logic as LLVM's @llvm.minimum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 8 and LLVM 9.
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);

                // To detect min(-0.0, 0.0), we check whether the integer
                // representations are equal. There's one other case where that
                // can happen: non-canonical NaNs. Here we unconditionally
                // canonicalize the NaNs. Note that this is a different
                // canonicalization from that which may be performed in the
                // v128_into_f32x4 self.function. That may canonicalize as F64x2 if
                // previous computations may have emitted F64x2 NaNs.
                let v1 =
                    canonicalize_nans(&self.builder, self.intrinsics, v1.as_basic_value_enum());
                let v2 =
                    canonicalize_nans(&self.builder, self.intrinsics, v2.as_basic_value_enum());
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());

                let v1_is_nan = self.builder.build_float_compare(
                    FloatPredicate::UNO,
                    v1,
                    self.intrinsics.f64x2_zero,
                    "nan",
                );
                let v2_is_not_nan = self.builder.build_float_compare(
                    FloatPredicate::ORD,
                    v2,
                    self.intrinsics.f64x2_zero,
                    "notnan",
                );
                let v1_repr = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let v2_repr = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let repr_ne =
                    self.builder
                        .build_int_compare(IntPredicate::NE, v1_repr, v2_repr, "");
                let float_eq = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let min_cmp = self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let negative_zero = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    self.intrinsics
                        .f64_ty
                        .const_float(-0.0)
                        .as_basic_value_enum(),
                    self.intrinsics.f64x2_ty,
                    "",
                );
                let v2 = self
                    .builder
                    .build_select(
                        self.builder.build_and(
                            self.builder.build_and(float_eq, repr_ne, ""),
                            v2_is_not_nan,
                            "",
                        ),
                        negative_zero,
                        v2,
                        "",
                    )
                    .into_vector_value();
                let res = self.builder.build_select(
                    self.builder.build_or(v1_is_nan, min_cmp, ""),
                    v1,
                    v2,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // Because inputs were canonicalized, we always produce
                // canonical NaN outputs. No pending NaN cleanup.
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::F32Max => {
                // This implements the same logic as LLVM's @llvm.maximum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 8 and LLVM 9.
                let (v1, v2) = self.state.pop2()?;

                // To detect min(-0.0, 0.0), we check whether the integer
                // representations are equal. There's one other case where that
                // can happen: non-canonical NaNs. Here we unconditionally
                // canonicalize the NaNs.
                let v1 = canonicalize_nans(&self.builder, self.intrinsics, v1);
                let v2 = canonicalize_nans(&self.builder, self.intrinsics, v2);

                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let v1_is_nan = self.builder.build_float_compare(
                    FloatPredicate::UNO,
                    v1,
                    self.intrinsics.f32_zero,
                    "nan",
                );
                let v2_is_not_nan = self.builder.build_float_compare(
                    FloatPredicate::ORD,
                    v2,
                    self.intrinsics.f32_zero,
                    "notnan",
                );
                let v1_repr = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i32_ty, "")
                    .into_int_value();
                let v2_repr = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i32_ty, "")
                    .into_int_value();
                let repr_ne =
                    self.builder
                        .build_int_compare(IntPredicate::NE, v1_repr, v2_repr, "");
                let float_eq = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let min_cmp = self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let v2 = self
                    .builder
                    .build_select(
                        self.builder.build_and(
                            self.builder.build_and(float_eq, repr_ne, ""),
                            v2_is_not_nan,
                            "",
                        ),
                        self.intrinsics.f32_zero,
                        v2,
                        "",
                    )
                    .into_float_value();
                let res = self.builder.build_select(
                    self.builder.build_or(v1_is_nan, min_cmp, ""),
                    v1,
                    v2,
                    "",
                );
                // Because inputs were canonicalized, we always produce
                // canonical NaN outputs. No pending NaN cleanup.
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::F64Max => {
                // This implements the same logic as LLVM's @llvm.maximum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 8 and LLVM 9.
                let (v1, v2) = self.state.pop2()?;

                // To detect min(-0.0, 0.0), we check whether the integer
                // representations are equal. There's one other case where that
                // can happen: non-canonical NaNs. Here we unconditionally
                // canonicalize the NaNs.
                let v1 = canonicalize_nans(&self.builder, self.intrinsics, v1);
                let v2 = canonicalize_nans(&self.builder, self.intrinsics, v2);

                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let v1_is_nan = self.builder.build_float_compare(
                    FloatPredicate::UNO,
                    v1,
                    self.intrinsics.f64_zero,
                    "nan",
                );
                let v2_is_not_nan = self.builder.build_float_compare(
                    FloatPredicate::ORD,
                    v2,
                    self.intrinsics.f64_zero,
                    "notnan",
                );
                let v1_repr = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i64_ty, "")
                    .into_int_value();
                let v2_repr = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i64_ty, "")
                    .into_int_value();
                let repr_ne =
                    self.builder
                        .build_int_compare(IntPredicate::NE, v1_repr, v2_repr, "");
                let float_eq = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let min_cmp = self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let v2 = self
                    .builder
                    .build_select(
                        self.builder.build_and(
                            self.builder.build_and(float_eq, repr_ne, ""),
                            v2_is_not_nan,
                            "",
                        ),
                        self.intrinsics.f64_zero,
                        v2,
                        "",
                    )
                    .into_float_value();
                let res = self.builder.build_select(
                    self.builder.build_or(v1_is_nan, min_cmp, ""),
                    v1,
                    v2,
                    "",
                );
                // Because inputs were canonicalized, we always produce
                // canonical NaN outputs. No pending NaN cleanup.
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::F32x4Max => {
                // This implements the same logic as LLVM's @llvm.maximum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 8 and LLVM 9.
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);

                // To detect min(-0.0, 0.0), we check whether the integer
                // representations are equal. There's one other case where that
                // can happen: non-canonical NaNs. Here we unconditionally
                // canonicalize the NaNs. Note that this is a different
                // canonicalization from that which may be performed in the
                // v128_into_f32x4 self.function. That may canonicalize as F64x2 if
                // previous computations may have emitted F64x2 NaNs.
                let v1 =
                    canonicalize_nans(&self.builder, self.intrinsics, v1.as_basic_value_enum());
                let v2 =
                    canonicalize_nans(&self.builder, self.intrinsics, v2.as_basic_value_enum());
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let v1_is_nan = self.builder.build_float_compare(
                    FloatPredicate::UNO,
                    v1,
                    self.intrinsics.f32x4_zero,
                    "nan",
                );
                let v2_is_not_nan = self.builder.build_float_compare(
                    FloatPredicate::ORD,
                    v2,
                    self.intrinsics.f32x4_zero,
                    "notnan",
                );
                let v1_repr = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let v2_repr = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let repr_ne =
                    self.builder
                        .build_int_compare(IntPredicate::NE, v1_repr, v2_repr, "");
                let float_eq = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let min_cmp = self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let zero = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    self.intrinsics.f32_zero.as_basic_value_enum(),
                    self.intrinsics.f32x4_ty,
                    "",
                );
                let v2 = self
                    .builder
                    .build_select(
                        self.builder.build_and(
                            self.builder.build_and(float_eq, repr_ne, ""),
                            v2_is_not_nan,
                            "",
                        ),
                        zero,
                        v2,
                        "",
                    )
                    .into_vector_value();
                let res = self.builder.build_select(
                    self.builder.build_or(v1_is_nan, min_cmp, ""),
                    v1,
                    v2,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // Because inputs were canonicalized, we always produce
                // canonical NaN outputs. No pending NaN cleanup.
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::F64x2Max => {
                // This implements the same logic as LLVM's @llvm.maximum
                // intrinsic would, but x86 lowering of that intrinsic
                // encounters a fatal error in LLVM 8 and LLVM 9.
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);

                // To detect min(-0.0, 0.0), we check whether the integer
                // representations are equal. There's one other case where that
                // can happen: non-canonical NaNs. Here we unconditionally
                // canonicalize the NaNs. Note that this is a different
                // canonicalization from that which may be performed in the
                // v128_into_f32x4 self.function. That may canonicalize as F64x2 if
                // previous computations may have emitted F64x2 NaNs.
                let v1 =
                    canonicalize_nans(&self.builder, self.intrinsics, v1.as_basic_value_enum());
                let v2 =
                    canonicalize_nans(&self.builder, self.intrinsics, v2.as_basic_value_enum());
                let (v1, v2) = (v1.into_vector_value(), v2.into_vector_value());
                let v1_is_nan = self.builder.build_float_compare(
                    FloatPredicate::UNO,
                    v1,
                    self.intrinsics.f64x2_zero,
                    "nan",
                );
                let v2_is_not_nan = self.builder.build_float_compare(
                    FloatPredicate::ORD,
                    v2,
                    self.intrinsics.f64x2_zero,
                    "notnan",
                );
                let v1_repr = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let v2_repr = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let repr_ne =
                    self.builder
                        .build_int_compare(IntPredicate::NE, v1_repr, v2_repr, "");
                let float_eq = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let min_cmp = self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let zero = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    self.intrinsics.f64_zero.as_basic_value_enum(),
                    self.intrinsics.f64x2_ty,
                    "",
                );
                let v2 = self
                    .builder
                    .build_select(
                        self.builder.build_and(
                            self.builder.build_and(float_eq, repr_ne, ""),
                            v2_is_not_nan,
                            "",
                        ),
                        zero,
                        v2,
                        "",
                    )
                    .into_vector_value();
                let res = self.builder.build_select(
                    self.builder.build_or(v1_is_nan, min_cmp, ""),
                    v1,
                    v2,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // Because inputs were canonicalized, we always produce
                // canonical NaN outputs. No pending NaN cleanup.
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::F32Ceil => {
                let (input, info) = self.state.pop1_extra()?;
                let res = self
                    .builder
                    .build_call(self.intrinsics.ceil_f32, &[input], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, info | ExtraInfo::pending_f32_nan());
            }
            Operator::F64Ceil => {
                let (input, info) = self.state.pop1_extra()?;
                let res = self
                    .builder
                    .build_call(self.intrinsics.ceil_f64, &[input], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, info | ExtraInfo::pending_f64_nan());
            }
            Operator::F32Floor => {
                let (input, info) = self.state.pop1_extra()?;
                let res = self
                    .builder
                    .build_call(self.intrinsics.floor_f32, &[input], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, info | ExtraInfo::pending_f32_nan());
            }
            Operator::F64Floor => {
                let (input, info) = self.state.pop1_extra()?;
                let res = self
                    .builder
                    .build_call(self.intrinsics.floor_f64, &[input], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, info | ExtraInfo::pending_f64_nan());
            }
            Operator::F32Trunc => {
                let (v, i) = self.state.pop1_extra()?;
                let res = self
                    .builder
                    .build_call(self.intrinsics.trunc_f32, &[v.as_basic_value_enum()], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, i | ExtraInfo::pending_f32_nan());
            }
            Operator::F64Trunc => {
                let (v, i) = self.state.pop1_extra()?;
                let res = self
                    .builder
                    .build_call(self.intrinsics.trunc_f64, &[v.as_basic_value_enum()], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, i | ExtraInfo::pending_f64_nan());
            }
            Operator::F32Nearest => {
                let (v, i) = self.state.pop1_extra()?;
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.nearbyint_f32,
                        &[v.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, i | ExtraInfo::pending_f32_nan());
            }
            Operator::F64Nearest => {
                let (v, i) = self.state.pop1_extra()?;
                let res = self
                    .builder
                    .build_call(
                        self.intrinsics.nearbyint_f64,
                        &[v.as_basic_value_enum()],
                        "",
                    )
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                self.state
                    .push1_extra(res, i | ExtraInfo::pending_f64_nan());
            }
            Operator::F32Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let res = self
                    .builder
                    .build_call(self.intrinsics.fabs_f32, &[v.as_basic_value_enum()], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                // The exact NaN returned by F32Abs is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F64Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let res = self
                    .builder
                    .build_call(self.intrinsics.fabs_f64, &[v.as_basic_value_enum()], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                // The exact NaN returned by F64Abs is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F32x4Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let v =
                    self.builder
                        .build_bitcast(v.into_int_value(), self.intrinsics.f32x4_ty, "");
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let res = self
                    .builder
                    .build_call(self.intrinsics.fabs_f32x4, &[v.as_basic_value_enum()], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // The exact NaN returned by F32x4Abs is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F64x2Abs => {
                let (v, i) = self.state.pop1_extra()?;
                let v =
                    self.builder
                        .build_bitcast(v.into_int_value(), self.intrinsics.f64x2_ty, "");
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let res = self
                    .builder
                    .build_call(self.intrinsics.fabs_f64x2, &[v], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // The exact NaN returned by F32x4Abs is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F32x4Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let v =
                    self.builder
                        .build_bitcast(v.into_int_value(), self.intrinsics.f32x4_ty, "");
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i)
                    .into_vector_value();
                let res = self.builder.build_float_neg(v, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // The exact NaN returned by F32x4Neg is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F64x2Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let v =
                    self.builder
                        .build_bitcast(v.into_int_value(), self.intrinsics.f64x2_ty, "");
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i)
                    .into_vector_value();
                let res = self.builder.build_float_neg(v, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                // The exact NaN returned by F64x2Neg is fully defined. Do not
                // adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F32Neg | Operator::F64Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i)
                    .into_float_value();
                let res = self.builder.build_float_neg(v, "");
                // The exact NaN returned by F32Neg and F64Neg are fully defined.
                // Do not adjust.
                self.state.push1_extra(res, i.strip_pending());
            }
            Operator::F32Copysign => {
                let ((mag, mag_info), (sgn, sgn_info)) = self.state.pop2_extra()?;
                let mag =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, mag, mag_info);
                let sgn =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, sgn, sgn_info);
                let res = self
                    .builder
                    .build_call(self.intrinsics.copysign_f32, &[mag, sgn], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                // The exact NaN returned by F32Copysign is fully defined.
                // Do not adjust.
                self.state.push1_extra(res, mag_info.strip_pending());
            }
            Operator::F64Copysign => {
                let ((mag, mag_info), (sgn, sgn_info)) = self.state.pop2_extra()?;
                let mag =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, mag, mag_info);
                let sgn =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, sgn, sgn_info);
                let res = self
                    .builder
                    .build_call(self.intrinsics.copysign_f64, &[mag, sgn], "")
                    .try_as_basic_value()
                    .left()
                    .unwrap();
                // The exact NaN returned by F32Copysign is fully defined.
                // Do not adjust.
                self.state.push1_extra(res, mag_info.strip_pending());
            }

            /***************************
             * Integer Comparison instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#integer-comparison-instructions
             ***************************/
            Operator::I32Eq | Operator::I64Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self.builder.build_int_compare(IntPredicate::EQ, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_compare(IntPredicate::EQ, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_compare(IntPredicate::EQ, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_compare(IntPredicate::EQ, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32Ne | Operator::I64Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self.builder.build_int_compare(IntPredicate::NE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_compare(IntPredicate::NE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_compare(IntPredicate::NE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self.builder.build_int_compare(IntPredicate::NE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32LtS | Operator::I64LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4LtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SLT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32LtU | Operator::I64LtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I8x16LtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8LtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4LtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::ULT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32LeS | Operator::I64LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4LeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SLE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32LeU | Operator::I64LeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::ULE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16LeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::ULE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8LeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::ULE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4LeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::ULE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32GtS | Operator::I64GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4GtS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32GtU | Operator::I64GtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16GtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8GtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4GtU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::UGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32GeS | Operator::I64GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I8x16GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4GeS => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::SGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32GeU | Operator::I64GeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let (v1, v2) = (v1.into_int_value(), v2.into_int_value());
                let cond = self
                    .builder
                    .build_int_compare(IntPredicate::UGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16GeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i8x16(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::UGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i8x16_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8GeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i16x8(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::UGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i16x8_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4GeU => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_i32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_int_compare(IntPredicate::UGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }

            /***************************
             * Floating-Point Comparison instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#floating-point-comparison-instructions
             ***************************/
            Operator::F32Eq | Operator::F64Eq => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::F32x4Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F64x2Eq => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F32Ne | Operator::F64Ne => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = self
                    .builder
                    .build_float_compare(FloatPredicate::UNE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::F32x4Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::UNE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F64x2Ne => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::UNE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F32Lt | Operator::F64Lt => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::F32x4Lt => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F64x2Lt => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F32Le | Operator::F64Le => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::F32x4Le => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F64x2Le => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F32Gt | Operator::F64Gt => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::F32x4Gt => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F64x2Gt => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F32Ge | Operator::F64Ge => {
                let (v1, v2) = self.state.pop2()?;
                let (v1, v2) = (v1.into_float_value(), v2.into_float_value());
                let cond = self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_z_extend(cond, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::F32x4Ge => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f32x4(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F64x2Ge => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let (v2, _) = v128_into_f64x2(&self.builder, self.intrinsics, v2, i2);
                let res = self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, v1, v2, "");
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i64x2_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }

            /***************************
             * Conversion instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#conversion-instructions
             ***************************/
            Operator::I32WrapI64 => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self
                    .builder
                    .build_int_truncate(v, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I64ExtendI32S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self
                    .builder
                    .build_int_s_extend(v, self.intrinsics.i64_ty, "");
                self.state.push1(res);
            }
            Operator::I64ExtendI32U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self
                    .builder
                    .build_int_z_extend(v, self.intrinsics.i64_ty, "");
                self.state.push1_extra(res, ExtraInfo::arithmetic_f64());
            }
            Operator::I32x4TruncSatF32x4S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self.trunc_sat(
                    self.intrinsics.f32x4_ty,
                    self.intrinsics.i32x4_ty,
                    -2147480000i32 as u32 as u64,
                    2147480000,
                    std::i32::MIN as u64,
                    std::i32::MAX as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I32x4TruncSatF32x4U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self.trunc_sat(
                    self.intrinsics.f32x4_ty,
                    self.intrinsics.i32x4_ty,
                    0,
                    4294960000,
                    std::u32::MIN as u64,
                    std::u32::MAX as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I64x2TruncSatF64x2S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self.trunc_sat(
                    self.intrinsics.f64x2_ty,
                    self.intrinsics.i64x2_ty,
                    std::i64::MIN as u64,
                    std::i64::MAX as u64,
                    std::i64::MIN as u64,
                    std::i64::MAX as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I64x2TruncSatF64x2U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self.trunc_sat(
                    self.intrinsics.f64x2_ty,
                    self.intrinsics.i64x2_ty,
                    std::u64::MIN,
                    std::u64::MAX,
                    std::u64::MIN,
                    std::u64::MAX,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I32TruncF32S => {
                let v1 = self.state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    0xcf000000, // -2147483600.0
                    0x4effffff, // 2147483500.0
                    v1,
                );
                let res = self
                    .builder
                    .build_float_to_signed_int(v1, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I32TruncF64S => {
                let v1 = self.state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    0xc1e00000001fffff, // -2147483648.9999995
                    0x41dfffffffffffff, // 2147483647.9999998
                    v1,
                );
                let res = self
                    .builder
                    .build_float_to_signed_int(v1, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I32TruncSatF32S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i32_ty,
                    LEF32_GEQ_I32_MIN,
                    GEF32_LEQ_I32_MAX,
                    std::i32::MIN as u32 as u64,
                    std::i32::MAX as u32 as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I32TruncSatF64S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i32_ty,
                    LEF64_GEQ_I32_MIN,
                    GEF64_LEQ_I32_MAX,
                    std::i32::MIN as u64,
                    std::i32::MAX as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I64TruncF32S => {
                let v1 = self.state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    0xdf000000, // -9223372000000000000.0
                    0x5effffff, // 9223371500000000000.0
                    v1,
                );
                let res = self
                    .builder
                    .build_float_to_signed_int(v1, self.intrinsics.i64_ty, "");
                self.state.push1(res);
            }
            Operator::I64TruncF64S => {
                let v1 = self.state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    0xc3e0000000000000, // -9223372036854776000.0
                    0x43dfffffffffffff, // 9223372036854775000.0
                    v1,
                );
                let res = self
                    .builder
                    .build_float_to_signed_int(v1, self.intrinsics.i64_ty, "");
                self.state.push1(res);
            }
            Operator::I64TruncSatF32S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i64_ty,
                    LEF32_GEQ_I64_MIN,
                    GEF32_LEQ_I64_MAX,
                    std::i64::MIN as u64,
                    std::i64::MAX as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I64TruncSatF64S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i64_ty,
                    LEF64_GEQ_I64_MIN,
                    GEF64_LEQ_I64_MAX,
                    std::i64::MIN as u64,
                    std::i64::MAX as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I32TruncF32U => {
                let v1 = self.state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    0xbf7fffff, // -0.99999994
                    0x4f7fffff, // 4294967000.0
                    v1,
                );
                let res = self
                    .builder
                    .build_float_to_unsigned_int(v1, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I32TruncF64U => {
                let v1 = self.state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    0xbfefffffffffffff, // -0.9999999999999999
                    0x41efffffffffffff, // 4294967295.9999995
                    v1,
                );
                let res = self
                    .builder
                    .build_float_to_unsigned_int(v1, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I32TruncSatF32U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i32_ty,
                    LEF32_GEQ_U32_MIN,
                    GEF32_LEQ_U32_MAX,
                    std::u32::MIN as u64,
                    std::u32::MAX as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I32TruncSatF64U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i32_ty,
                    LEF64_GEQ_U32_MIN,
                    GEF64_LEQ_U32_MAX,
                    std::u32::MIN as u64,
                    std::u32::MAX as u64,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I64TruncF32U => {
                let v1 = self.state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    0xbf7fffff, // -0.99999994
                    0x5f7fffff, // 18446743000000000000.0
                    v1,
                );
                let res = self
                    .builder
                    .build_float_to_unsigned_int(v1, self.intrinsics.i64_ty, "");
                self.state.push1(res);
            }
            Operator::I64TruncF64U => {
                let v1 = self.state.pop1()?.into_float_value();
                trap_if_not_representable_as_int(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    0xbfefffffffffffff, // -0.9999999999999999
                    0x43efffffffffffff, // 18446744073709550000.0
                    v1,
                );
                let res = self
                    .builder
                    .build_float_to_unsigned_int(v1, self.intrinsics.i64_ty, "");
                self.state.push1(res);
            }
            Operator::I64TruncSatF32U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i64_ty,
                    LEF32_GEQ_U64_MIN,
                    GEF32_LEQ_U64_MAX,
                    std::u64::MIN,
                    std::u64::MAX,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::I64TruncSatF64U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_float_value();
                let res = self.trunc_sat_scalar(
                    self.intrinsics.i64_ty,
                    LEF64_GEQ_U64_MIN,
                    GEF64_LEQ_U64_MAX,
                    std::u64::MIN,
                    std::u64::MAX,
                    v,
                    "",
                );
                self.state.push1(res);
            }
            Operator::F32DemoteF64 => {
                let v = self.state.pop1()?;
                let v = v.into_float_value();
                let res = self
                    .builder
                    .build_float_trunc(v, self.intrinsics.f32_ty, "");
                self.state.push1_extra(res, ExtraInfo::pending_f32_nan());
            }
            Operator::F64PromoteF32 => {
                let v = self.state.pop1()?;
                let v = v.into_float_value();
                let res = self.builder.build_float_ext(v, self.intrinsics.f64_ty, "");
                self.state.push1_extra(res, ExtraInfo::pending_f64_nan());
            }
            Operator::F32ConvertI32S | Operator::F32ConvertI64S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self
                    .builder
                    .build_signed_int_to_float(v, self.intrinsics.f32_ty, "");
                self.state.push1(res);
            }
            Operator::F64ConvertI32S | Operator::F64ConvertI64S => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self
                    .builder
                    .build_signed_int_to_float(v, self.intrinsics.f64_ty, "");
                self.state.push1(res);
            }
            Operator::F32ConvertI32U | Operator::F32ConvertI64U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self
                    .builder
                    .build_unsigned_int_to_float(v, self.intrinsics.f32_ty, "");
                self.state.push1(res);
            }
            Operator::F64ConvertI32U | Operator::F64ConvertI64U => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let v = v.into_int_value();
                let res = self
                    .builder
                    .build_unsigned_int_to_float(v, self.intrinsics.f64_ty, "");
                self.state.push1(res);
            }
            Operator::F32x4ConvertI32x4S => {
                let v = self.state.pop1()?;
                let v = self
                    .builder
                    .build_bitcast(v, self.intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = self
                    .builder
                    .build_signed_int_to_float(v, self.intrinsics.f32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F32x4ConvertI32x4U => {
                let v = self.state.pop1()?;
                let v = self
                    .builder
                    .build_bitcast(v, self.intrinsics.i32x4_ty, "")
                    .into_vector_value();
                let res = self
                    .builder
                    .build_unsigned_int_to_float(v, self.intrinsics.f32x4_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F64x2ConvertI64x2S => {
                let v = self.state.pop1()?;
                let v = self
                    .builder
                    .build_bitcast(v, self.intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let res = self
                    .builder
                    .build_signed_int_to_float(v, self.intrinsics.f64x2_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::F64x2ConvertI64x2U => {
                let v = self.state.pop1()?;
                let v = self
                    .builder
                    .build_bitcast(v, self.intrinsics.i64x2_ty, "")
                    .into_vector_value();
                let res = self
                    .builder
                    .build_unsigned_int_to_float(v, self.intrinsics.f64x2_ty, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32ReinterpretF32 => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let ret = self.builder.build_bitcast(v, self.intrinsics.i32_ty, "");
                self.state.push1_extra(ret, ExtraInfo::arithmetic_f32());
            }
            Operator::I64ReinterpretF64 => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let ret = self.builder.build_bitcast(v, self.intrinsics.i64_ty, "");
                self.state.push1_extra(ret, ExtraInfo::arithmetic_f64());
            }
            Operator::F32ReinterpretI32 => {
                let (v, i) = self.state.pop1_extra()?;
                let ret = self.builder.build_bitcast(v, self.intrinsics.f32_ty, "");
                self.state.push1_extra(ret, i);
            }
            Operator::F64ReinterpretI64 => {
                let (v, i) = self.state.pop1_extra()?;
                let ret = self.builder.build_bitcast(v, self.intrinsics.f64_ty, "");
                self.state.push1_extra(ret, i);
            }

            /***************************
             * Sign-extension operators.
             * https://github.com/WebAssembly/sign-extension-ops/blob/master/proposals/sign-extension-ops/Overview.md
             ***************************/
            Operator::I32Extend8S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let extended_value =
                    self.builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i32_ty, "");
                self.state.push1(extended_value);
            }
            Operator::I32Extend16S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let extended_value =
                    self.builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i32_ty, "");
                self.state.push1(extended_value);
            }
            Operator::I64Extend8S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let extended_value =
                    self.builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i64_ty, "");
                self.state.push1(extended_value);
            }
            Operator::I64Extend16S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let extended_value =
                    self.builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i64_ty, "");
                self.state.push1(extended_value);
            }
            Operator::I64Extend32S => {
                let value = self.state.pop1()?.into_int_value();
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let extended_value =
                    self.builder
                        .build_int_s_extend(narrow_value, self.intrinsics.i64_ty, "");
                self.state.push1(extended_value);
            }

            /***************************
             * Load and Store instructions.
             * https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#load-and-store-instructions
             ***************************/
            Operator::I32Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                let result = self.builder.build_load(effective_address, "");
                result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    result.as_instruction_value().unwrap(),
                );
                self.state.push1(result);
            }
            Operator::I64Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                let result = self.builder.build_load(effective_address, "");
                result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    result.as_instruction_value().unwrap(),
                );
                self.state.push1(result);
            }
            Operator::F32Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.f32_ptr_ty,
                    offset,
                    4,
                )?;
                let result = self.builder.build_load(effective_address, "");
                result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    result.as_instruction_value().unwrap(),
                );
                self.state.push1(result);
            }
            Operator::F64Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.f64_ptr_ty,
                    offset,
                    8,
                )?;
                let result = self.builder.build_load(effective_address, "");
                result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    result.as_instruction_value().unwrap(),
                );
                self.state.push1(result);
            }
            Operator::V128Load { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i128_ptr_ty,
                    offset,
                    16,
                )?;
                let result = self.builder.build_load(effective_address, "");
                result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    result.as_instruction_value().unwrap(),
                );
                self.state.push1(result);
            }

            Operator::I32Store { ref memarg } => {
                let value = self.state.pop1()?;
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                let store = self.builder.build_store(effective_address, value);
                store.set_alignment(1).unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I64Store { ref memarg } => {
                let value = self.state.pop1()?;
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                let store = self.builder.build_store(effective_address, value);
                store.set_alignment(1).unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::F32Store { ref memarg } => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.f32_ptr_ty,
                    offset,
                    4,
                )?;
                let store = self.builder.build_store(effective_address, v);
                store.set_alignment(1).unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::F64Store { ref memarg } => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.f64_ptr_ty,
                    offset,
                    8,
                )?;
                let store = self.builder.build_store(effective_address, v);
                store.set_alignment(1).unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::V128Store { ref memarg } => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i);
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i128_ptr_ty,
                    offset,
                    16,
                )?;
                let store = self.builder.build_store(effective_address, v);
                store.set_alignment(1).unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I32Load8S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_result = self.builder.build_load(effective_address, "");
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result = self.builder.build_int_s_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i32_ty,
                    "",
                );
                self.state.push1(result);
            }
            Operator::I32Load16S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_result = self.builder.build_load(effective_address, "");
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result = self.builder.build_int_s_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i32_ty,
                    "",
                );
                self.state.push1(result);
            }
            Operator::I64Load8S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_result = self
                    .builder
                    .build_load(effective_address, "")
                    .into_int_value();
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result =
                    self.builder
                        .build_int_s_extend(narrow_result, self.intrinsics.i64_ty, "");
                self.state.push1(result);
            }
            Operator::I64Load16S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_result = self
                    .builder
                    .build_load(effective_address, "")
                    .into_int_value();
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result =
                    self.builder
                        .build_int_s_extend(narrow_result, self.intrinsics.i64_ty, "");
                self.state.push1(result);
            }
            Operator::I64Load32S { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                let narrow_result = self.builder.build_load(effective_address, "");
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result = self.builder.build_int_s_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i64_ty,
                    "",
                );
                self.state.push1(result);
            }

            Operator::I32Load8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_result = self.builder.build_load(effective_address, "");
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result = self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i32_ty,
                    "",
                );
                self.state.push1_extra(result, ExtraInfo::arithmetic_f32());
            }
            Operator::I32Load16U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_result = self.builder.build_load(effective_address, "");
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result = self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i32_ty,
                    "",
                );
                self.state.push1_extra(result, ExtraInfo::arithmetic_f32());
            }
            Operator::I64Load8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_result = self.builder.build_load(effective_address, "");
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result = self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i64_ty,
                    "",
                );
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I64Load16U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_result = self.builder.build_load(effective_address, "");
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result = self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i64_ty,
                    "",
                );
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I64Load32U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                let narrow_result = self.builder.build_load(effective_address, "");
                narrow_result
                    .as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    narrow_result.as_instruction_value().unwrap(),
                );
                let result = self.builder.build_int_z_extend(
                    narrow_result.into_int_value(),
                    self.intrinsics.i64_ty,
                    "",
                );
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }

            Operator::I32Store8 { ref memarg } | Operator::I64Store8 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let store = self.builder.build_store(effective_address, narrow_value);
                store.set_alignment(1).unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I32Store16 { ref memarg } | Operator::I64Store16 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let store = self.builder.build_store(effective_address, narrow_value);
                store.set_alignment(1).unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I64Store32 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let store = self.builder.build_store(effective_address, narrow_value);
                store.set_alignment(1).unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I8x16Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_i8x16(&self.builder, self.intrinsics, v, i);
                let res = self.builder.build_int_sub(v.get_type().const_zero(), v, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_i16x8(&self.builder, self.intrinsics, v, i);
                let res = self.builder.build_int_sub(v.get_type().const_zero(), v, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_i32x4(&self.builder, self.intrinsics, v, i);
                let res = self.builder.build_int_sub(v.get_type().const_zero(), v, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I64x2Neg => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_i64x2(&self.builder, self.intrinsics, v, i);
                let res = self.builder.build_int_sub(v.get_type().const_zero(), v, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::V128Not => {
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i)
                    .into_int_value();
                let res = self.builder.build_not(v, "");
                self.state.push1(res);
            }
            Operator::I8x16AnyTrue
            | Operator::I16x8AnyTrue
            | Operator::I32x4AnyTrue
            | Operator::I64x2AnyTrue => {
                // Skip canonicalization, it never changes non-zero values to zero or vice versa.
                let v = self.state.pop1()?.into_int_value();
                let res = self.builder.build_int_compare(
                    IntPredicate::NE,
                    v,
                    v.get_type().const_zero(),
                    "",
                );
                let res = self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16AllTrue
            | Operator::I16x8AllTrue
            | Operator::I32x4AllTrue
            | Operator::I64x2AllTrue => {
                let vec_ty = match op {
                    Operator::I8x16AllTrue => self.intrinsics.i8x16_ty,
                    Operator::I16x8AllTrue => self.intrinsics.i16x8_ty,
                    Operator::I32x4AllTrue => self.intrinsics.i32x4_ty,
                    Operator::I64x2AllTrue => self.intrinsics.i64x2_ty,
                    _ => unreachable!(),
                };
                let (v, i) = self.state.pop1_extra()?;
                let v = apply_pending_canonicalization(&self.builder, self.intrinsics, v, i)
                    .into_int_value();
                let lane_int_ty = self.context.custom_width_int_type(vec_ty.get_size());
                let vec = self
                    .builder
                    .build_bitcast(v, vec_ty, "vec")
                    .into_vector_value();
                let mask = self.builder.build_int_compare(
                    IntPredicate::NE,
                    vec,
                    vec_ty.const_zero(),
                    "mask",
                );
                let cmask = self
                    .builder
                    .build_bitcast(mask, lane_int_ty, "cmask")
                    .into_int_value();
                let res = self.builder.build_int_compare(
                    IntPredicate::EQ,
                    cmask,
                    lane_int_ty.const_int(std::u64::MAX, true),
                    "",
                );
                let res = self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, "");
                self.state.push1_extra(
                    res,
                    ExtraInfo::arithmetic_f32() | ExtraInfo::arithmetic_f64(),
                );
            }
            Operator::I8x16ExtractLaneS { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_i8x16(&self.builder, self.intrinsics, v, i);
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self
                    .builder
                    .build_extract_element(v, idx, "")
                    .into_int_value();
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I8x16ExtractLaneU { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_i8x16(&self.builder, self.intrinsics, v, i);
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self
                    .builder
                    .build_extract_element(v, idx, "")
                    .into_int_value();
                let res = self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, "");
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I16x8ExtractLaneS { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_i16x8(&self.builder, self.intrinsics, v, i);
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self
                    .builder
                    .build_extract_element(v, idx, "")
                    .into_int_value();
                let res = self
                    .builder
                    .build_int_s_extend(res, self.intrinsics.i32_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8ExtractLaneU { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, _) = v128_into_i16x8(&self.builder, self.intrinsics, v, i);
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self
                    .builder
                    .build_extract_element(v, idx, "")
                    .into_int_value();
                let res = self
                    .builder
                    .build_int_z_extend(res, self.intrinsics.i32_ty, "");
                self.state.push1_extra(res, ExtraInfo::arithmetic_f32());
            }
            Operator::I32x4ExtractLane { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = v128_into_i32x4(&self.builder, self.intrinsics, v, i);
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_extract_element(v, idx, "");
                self.state.push1_extra(res, i);
            }
            Operator::I64x2ExtractLane { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = v128_into_i64x2(&self.builder, self.intrinsics, v, i);
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_extract_element(v, idx, "");
                self.state.push1_extra(res, i);
            }
            Operator::F32x4ExtractLane { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = v128_into_f32x4(&self.builder, self.intrinsics, v, i);
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_extract_element(v, idx, "");
                self.state.push1_extra(res, i);
            }
            Operator::F64x2ExtractLane { lane } => {
                let (v, i) = self.state.pop1_extra()?;
                let (v, i) = v128_into_f64x2(&self.builder, self.intrinsics, v, i);
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_extract_element(v, idx, "");
                self.state.push1_extra(res, i);
            }
            Operator::I8x16ReplaceLane { lane } => {
                let ((v1, i1), (v2, _)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i8x16(&self.builder, self.intrinsics, v1, i1);
                let v2 = v2.into_int_value();
                let v2 = self.builder.build_int_cast(v2, self.intrinsics.i8_ty, "");
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_insert_element(v1, v2, idx, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I16x8ReplaceLane { lane } => {
                let ((v1, i1), (v2, _)) = self.state.pop2_extra()?;
                let (v1, _) = v128_into_i16x8(&self.builder, self.intrinsics, v1, i1);
                let v2 = v2.into_int_value();
                let v2 = self.builder.build_int_cast(v2, self.intrinsics.i16_ty, "");
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_insert_element(v1, v2, idx, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::I32x4ReplaceLane { lane } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_i32x4(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let i2 = i2.strip_pending();
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_insert_element(v1, v2, idx, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state
                    .push1_extra(res, i1 & i2 & ExtraInfo::arithmetic_f32());
            }
            Operator::I64x2ReplaceLane { lane } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_i64x2(&self.builder, self.intrinsics, v1, i1);
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = v2.into_int_value();
                let i2 = i2.strip_pending();
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_insert_element(v1, v2, idx, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state
                    .push1_extra(res, i1 & i2 & ExtraInfo::arithmetic_f64());
            }
            Operator::F32x4ReplaceLane { lane } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_f32x4(&self.builder, self.intrinsics, v1, i1);
                let push_pending_f32_nan_to_result =
                    i1.has_pending_f32_nan() && i2.has_pending_f32_nan();
                let (v1, v2) = if !push_pending_f32_nan_to_result {
                    (
                        apply_pending_canonicalization(
                            &self.builder,
                            self.intrinsics,
                            v1.as_basic_value_enum(),
                            i1,
                        )
                        .into_vector_value(),
                        apply_pending_canonicalization(
                            &self.builder,
                            self.intrinsics,
                            v2.as_basic_value_enum(),
                            i2,
                        )
                        .into_float_value(),
                    )
                } else {
                    (v1, v2.into_float_value())
                };
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_insert_element(v1, v2, idx, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                let info = if push_pending_f32_nan_to_result {
                    ExtraInfo::pending_f32_nan()
                } else {
                    i1.strip_pending() & i2.strip_pending()
                };
                self.state.push1_extra(res, info);
            }
            Operator::F64x2ReplaceLane { lane } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let (v1, i1) = v128_into_f64x2(&self.builder, self.intrinsics, v1, i1);
                let push_pending_f64_nan_to_result =
                    i1.has_pending_f64_nan() && i2.has_pending_f64_nan();
                let (v1, v2) = if !push_pending_f64_nan_to_result {
                    (
                        apply_pending_canonicalization(
                            &self.builder,
                            self.intrinsics,
                            v1.as_basic_value_enum(),
                            i1,
                        )
                        .into_vector_value(),
                        apply_pending_canonicalization(
                            &self.builder,
                            self.intrinsics,
                            v2.as_basic_value_enum(),
                            i2,
                        )
                        .into_float_value(),
                    )
                } else {
                    (v1, v2.into_float_value())
                };
                let idx = self.intrinsics.i32_ty.const_int(lane.into(), false);
                let res = self.builder.build_insert_element(v1, v2, idx, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                let info = if push_pending_f64_nan_to_result {
                    ExtraInfo::pending_f64_nan()
                } else {
                    i1.strip_pending() & i2.strip_pending()
                };
                self.state.push1_extra(res, info);
            }
            Operator::V8x16Swizzle => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v1 = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let lanes = self.intrinsics.i8_ty.const_int(16, false);
                let lanes = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    lanes.as_basic_value_enum(),
                    self.intrinsics.i8x16_ty,
                    "",
                );
                let mut res = self.intrinsics.i8x16_ty.get_undef();
                let idx_out_of_range = self.builder.build_int_compare(
                    IntPredicate::UGE,
                    v2,
                    lanes,
                    "idx_out_of_range",
                );
                let idx_clamped = self
                    .builder
                    .build_select(
                        idx_out_of_range,
                        self.intrinsics.i8x16_ty.const_zero(),
                        v2,
                        "idx_clamped",
                    )
                    .into_vector_value();
                for i in 0..16 {
                    let idx = self
                        .builder
                        .build_extract_element(
                            idx_clamped,
                            self.intrinsics.i32_ty.const_int(i, false),
                            "idx",
                        )
                        .into_int_value();
                    let replace_with_zero = self
                        .builder
                        .build_extract_element(
                            idx_out_of_range,
                            self.intrinsics.i32_ty.const_int(i, false),
                            "replace_with_zero",
                        )
                        .into_int_value();
                    let elem = self
                        .builder
                        .build_extract_element(v1, idx, "elem")
                        .into_int_value();
                    let elem_or_zero = self.builder.build_select(
                        replace_with_zero,
                        self.intrinsics.i8_zero,
                        elem,
                        "elem_or_zero",
                    );
                    res = self.builder.build_insert_element(
                        res,
                        elem_or_zero,
                        self.intrinsics.i32_ty.const_int(i, false),
                        "",
                    );
                }
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::V8x16Shuffle { lanes } => {
                let ((v1, i1), (v2, i2)) = self.state.pop2_extra()?;
                let v1 = apply_pending_canonicalization(&self.builder, self.intrinsics, v1, i1);
                let v1 = self
                    .builder
                    .build_bitcast(v1, self.intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let v2 = apply_pending_canonicalization(&self.builder, self.intrinsics, v2, i2);
                let v2 = self
                    .builder
                    .build_bitcast(v2, self.intrinsics.i8x16_ty, "")
                    .into_vector_value();
                let mask = VectorType::const_vector(
                    lanes
                        .iter()
                        .map(|l| self.intrinsics.i32_ty.const_int((*l).into(), false))
                        .collect::<Vec<IntValue>>()
                        .as_slice(),
                );
                let res = self.builder.build_shuffle_vector(v1, v2, mask, "");
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::V8x16LoadSplat { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                let elem = self.builder.build_load(effective_address, "");
                elem.as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    elem.as_instruction_value().unwrap(),
                );
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    elem,
                    self.intrinsics.i8x16_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::V16x8LoadSplat { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                let elem = self.builder.build_load(effective_address, "");
                elem.as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    elem.as_instruction_value().unwrap(),
                );
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    elem,
                    self.intrinsics.i16x8_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::V32x4LoadSplat { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                let elem = self.builder.build_load(effective_address, "");
                elem.as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    elem.as_instruction_value().unwrap(),
                );
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    elem,
                    self.intrinsics.i32x4_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::V64x2LoadSplat { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                let elem = self.builder.build_load(effective_address, "");
                elem.as_instruction_value()
                    .unwrap()
                    .set_alignment(1)
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    elem.as_instruction_value().unwrap(),
                );
                let res = splat_vector(
                    &self.builder,
                    self.intrinsics,
                    elem,
                    self.intrinsics.i64x2_ty,
                    "",
                );
                let res = self.builder.build_bitcast(res, self.intrinsics.i128_ty, "");
                self.state.push1(res);
            }
            Operator::AtomicFence { flags: _ } => {
                // Fence is a nop.
                //
                // Fence was added to preserve information about fences from
                // source languages. If in the future Wasm extends the memory
                // model, and if we hadn't recorded what fences used to be there,
                // it would lead to data races that weren't present in the
                // original source language.
            }
            Operator::I32AtomicLoad { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let result = self.builder.build_load(effective_address, "");
                let load = result.as_instruction_value().unwrap();
                load.set_alignment(4).unwrap();
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), load);
                self.state.push1(result);
            }
            Operator::I64AtomicLoad { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let result = self.builder.build_load(effective_address, "");
                let load = result.as_instruction_value().unwrap();
                load.set_alignment(8).unwrap();
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), load);
                self.state.push1(result);
            }
            Operator::I32AtomicLoad8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_result = self
                    .builder
                    .build_load(effective_address, "")
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                load.set_alignment(1).unwrap();
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), load);
                let result =
                    self.builder
                        .build_int_z_extend(narrow_result, self.intrinsics.i32_ty, "");
                self.state.push1_extra(result, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicLoad16U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_result = self
                    .builder
                    .build_load(effective_address, "")
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                load.set_alignment(2).unwrap();
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), load);
                let result =
                    self.builder
                        .build_int_z_extend(narrow_result, self.intrinsics.i32_ty, "");
                self.state.push1_extra(result, ExtraInfo::arithmetic_f32());
            }
            Operator::I64AtomicLoad8U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_result = self
                    .builder
                    .build_load(effective_address, "")
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                load.set_alignment(1).unwrap();
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), load);
                let result =
                    self.builder
                        .build_int_z_extend(narrow_result, self.intrinsics.i64_ty, "");
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicLoad16U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_result = self
                    .builder
                    .build_load(effective_address, "")
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                load.set_alignment(2).unwrap();
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), load);
                let result =
                    self.builder
                        .build_int_z_extend(narrow_result, self.intrinsics.i64_ty, "");
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicLoad32U { ref memarg } => {
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_result = self
                    .builder
                    .build_load(effective_address, "")
                    .into_int_value();
                let load = narrow_result.as_instruction_value().unwrap();
                load.set_alignment(4).unwrap();
                load.set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), load);
                let result =
                    self.builder
                        .build_int_z_extend(narrow_result, self.intrinsics.i64_ty, "");
                self.state.push1_extra(result, ExtraInfo::arithmetic_f64());
            }
            Operator::I32AtomicStore { ref memarg } => {
                let value = self.state.pop1()?;
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let store = self.builder.build_store(effective_address, value);
                store.set_alignment(4).unwrap();
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I64AtomicStore { ref memarg } => {
                let value = self.state.pop1()?;
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let store = self.builder.build_store(effective_address, value);
                store.set_alignment(8).unwrap();
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I32AtomicStore8 { ref memarg } | Operator::I64AtomicStore8 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let store = self.builder.build_store(effective_address, narrow_value);
                store.set_alignment(1).unwrap();
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I32AtomicStore16 { ref memarg }
            | Operator::I64AtomicStore16 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let store = self.builder.build_store(effective_address, narrow_value);
                store.set_alignment(2).unwrap();
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I64AtomicStore32 { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let store = self.builder.build_store(effective_address, narrow_value);
                store.set_alignment(4).unwrap();
                store
                    .set_atomic_ordering(AtomicOrdering::SequentiallyConsistent)
                    .unwrap();
                tbaa_label(&self.module, self.intrinsics, "memory 0".into(), store);
            }
            Operator::I32AtomicRmw8AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwAdd { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32AddU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwAdd { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Add,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwSub { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I64AtomicRmw16SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32SubU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwSub { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Sub,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwAnd { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32AndU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwAnd { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::And,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwOr { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I64AtomicRmw8OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32OrU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwOr { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Or,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwXor { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32XorU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwXor { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xor,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwXchg { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32XchgU { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_value =
                    self.builder
                        .build_int_truncate(value, self.intrinsics.i32_ty, "");
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        narrow_value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwXchg { ref memarg } => {
                let value = self.state.pop1()?.into_int_value();
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_atomicrmw(
                        AtomicRMWBinOp::Xchg,
                        effective_address,
                        value,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                self.state.push1(old);
            }
            Operator::I32AtomicRmw8CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, cmp, cmp_info);
                let new =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, new, new_info);
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_cmp = self
                    .builder
                    .build_int_truncate(cmp, self.intrinsics.i8_ty, "");
                let narrow_new = self
                    .builder
                    .build_int_truncate(new, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmw16CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, cmp, cmp_info);
                let new =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, new, new_info);
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_cmp = self
                    .builder
                    .build_int_truncate(cmp, self.intrinsics.i16_ty, "");
                let narrow_new = self
                    .builder
                    .build_int_truncate(new, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i32_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f32());
            }
            Operator::I32AtomicRmwCmpxchg { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, cmp, cmp_info);
                let new =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, new, new_info);
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        cmp,
                        new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self.builder.build_extract_value(old, 0, "").unwrap();
                self.state.push1(old);
            }
            Operator::I64AtomicRmw8CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, cmp, cmp_info);
                let new =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, new, new_info);
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i8_ptr_ty,
                    offset,
                    1,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_cmp = self
                    .builder
                    .build_int_truncate(cmp, self.intrinsics.i8_ty, "");
                let narrow_new = self
                    .builder
                    .build_int_truncate(new, self.intrinsics.i8_ty, "");
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw16CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, cmp, cmp_info);
                let new =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, new, new_info);
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i16_ptr_ty,
                    offset,
                    2,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_cmp = self
                    .builder
                    .build_int_truncate(cmp, self.intrinsics.i16_ty, "");
                let narrow_new = self
                    .builder
                    .build_int_truncate(new, self.intrinsics.i16_ty, "");
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmw32CmpxchgU { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, cmp, cmp_info);
                let new =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, new, new_info);
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i32_ptr_ty,
                    offset,
                    4,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let narrow_cmp = self
                    .builder
                    .build_int_truncate(cmp, self.intrinsics.i32_ty, "");
                let narrow_new = self
                    .builder
                    .build_int_truncate(new, self.intrinsics.i32_ty, "");
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        narrow_cmp,
                        narrow_new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self
                    .builder
                    .build_extract_value(old, 0, "")
                    .unwrap()
                    .into_int_value();
                let old = self
                    .builder
                    .build_int_z_extend(old, self.intrinsics.i64_ty, "");
                self.state.push1_extra(old, ExtraInfo::arithmetic_f64());
            }
            Operator::I64AtomicRmwCmpxchg { ref memarg } => {
                let ((cmp, cmp_info), (new, new_info)) = self.state.pop2_extra()?;
                let cmp =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, cmp, cmp_info);
                let new =
                    apply_pending_canonicalization(&self.builder, self.intrinsics, new, new_info);
                let (cmp, new) = (cmp.into_int_value(), new.into_int_value());
                let offset = self.state.pop1()?.into_int_value();
                let effective_address = self.resolve_memory_ptr(
                    &self.memory_plans,
                    memarg,
                    self.intrinsics.i64_ptr_ty,
                    offset,
                    8,
                )?;
                trap_if_misaligned(
                    &self.builder,
                    self.intrinsics,
                    self.context,
                    &self.function,
                    memarg,
                    effective_address,
                );
                let old = self
                    .builder
                    .build_cmpxchg(
                        effective_address,
                        cmp,
                        new,
                        AtomicOrdering::SequentiallyConsistent,
                        AtomicOrdering::SequentiallyConsistent,
                    )
                    .unwrap();
                tbaa_label(
                    &self.module,
                    self.intrinsics,
                    "memory 0".into(),
                    old.as_instruction_value().unwrap(),
                );
                let old = self.builder.build_extract_value(old, 0, "").unwrap();
                self.state.push1(old);
            }

            Operator::MemoryGrow { reserved } => {
                let mem_index = MemoryIndex::from_u32(reserved);
                let delta = self.state.pop1()?;
                let (grow_fn, grow_fn_ty) =
                    if let Some(local_mem_index) = module.local_memory_index(mem_index) {
                        (
                            VMBuiltinFunctionIndex::get_memory32_grow_index(),
                            self.intrinsics.memory32_grow_ptr_ty,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory32_grow_index(),
                            self.intrinsics.imported_memory32_grow_ptr_ty,
                        )
                    };
                let offset = self.vmoffsets.vmctx_builtin_function(grow_fn);
                let offset = self.intrinsics.i32_ty.const_int(offset.into(), false);
                let grow_fn_ptr_ptr = unsafe { self.builder.build_gep(*vmctx, &[offset], "") };

                let grow_fn_ptr_ptr = self
                    .builder
                    .build_bitcast(
                        grow_fn_ptr_ptr,
                        grow_fn_ty.ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into_pointer_value();
                let grow_fn_ptr = self
                    .builder
                    .build_load(grow_fn_ptr_ptr, "")
                    .into_pointer_value();
                let grow = self.builder.build_call(
                    grow_fn_ptr,
                    &[
                        vmctx.as_basic_value_enum(),
                        delta,
                        self.intrinsics
                            .i32_ty
                            .const_int(reserved.into(), false)
                            .as_basic_value_enum(),
                    ],
                    "",
                );
                self.state.push1(grow.try_as_basic_value().left().unwrap());
            }
            Operator::MemorySize { reserved } => {
                let mem_index = MemoryIndex::from_u32(reserved);
                let (size_fn, size_fn_ty) =
                    if let Some(local_mem_index) = module.local_memory_index(mem_index) {
                        (
                            VMBuiltinFunctionIndex::get_memory32_size_index(),
                            self.intrinsics.memory32_size_ptr_ty,
                        )
                    } else {
                        (
                            VMBuiltinFunctionIndex::get_imported_memory32_size_index(),
                            self.intrinsics.imported_memory32_size_ptr_ty,
                        )
                    };
                let offset = self.vmoffsets.vmctx_builtin_function(size_fn);
                let offset = self.intrinsics.i32_ty.const_int(offset.into(), false);
                let size_fn_ptr_ptr = unsafe { self.builder.build_gep(*vmctx, &[offset], "") };

                let size_fn_ptr_ptr = self
                    .builder
                    .build_bitcast(
                        size_fn_ptr_ptr,
                        size_fn_ty.ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into_pointer_value();
                let size_fn_ptr = self
                    .builder
                    .build_load(size_fn_ptr_ptr, "")
                    .into_pointer_value();
                let size = self.builder.build_call(
                    size_fn_ptr,
                    &[
                        vmctx.as_basic_value_enum(),
                        self.intrinsics
                            .i32_ty
                            .const_int(reserved.into(), false)
                            .as_basic_value_enum(),
                    ],
                    "",
                );
                self.state.push1(size.try_as_basic_value().left().unwrap());
            }
            _ => {
                return Err(CompileError::Codegen(format!(
                    "Operator {:?} unimplemented",
                    op
                )));
            }
        }

        Ok(())
    }
}

fn is_f32_arithmetic(bits: u32) -> bool {
    // Mask off sign bit.
    let bits = bits & 0x7FFF_FFFF;
    bits < 0x7FC0_0000
}

fn is_f64_arithmetic(bits: u64) -> bool {
    // Mask off sign bit.
    let bits = bits & 0x7FFF_FFFF_FFFF_FFFF;
    bits < 0x7FF8_0000_0000_0000
}

// Constants for the bounds of truncation operations. These are the least or
// greatest exact floats in either f32 or f64 representation
// greater-than-or-equal-to (for least) or less-than-or-equal-to (for greatest)
// the i32 or i64 or u32 or u64 min (for least) or max (for greatest), when
// rounding towards zero.

/// Least Exact Float (32 bits) greater-than-or-equal-to i32::MIN when rounding towards zero.
const LEF32_GEQ_I32_MIN: u64 = std::i32::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to i32::MAX when rounding towards zero.
const GEF32_LEQ_I32_MAX: u64 = 2147483520; // bits as f32: 0x4eff_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to i32::MIN when rounding towards zero.
const LEF64_GEQ_I32_MIN: u64 = std::i32::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to i32::MAX when rounding towards zero.
const GEF64_LEQ_I32_MAX: u64 = std::i32::MAX as u64;
/// Least Exact Float (32 bits) greater-than-or-equal-to u32::MIN when rounding towards zero.
const LEF32_GEQ_U32_MIN: u64 = std::u32::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to u32::MAX when rounding towards zero.
const GEF32_LEQ_U32_MAX: u64 = 4294967040; // bits as f32: 0x4f7f_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to u32::MIN when rounding towards zero.
const LEF64_GEQ_U32_MIN: u64 = std::u32::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to u32::MAX when rounding towards zero.
const GEF64_LEQ_U32_MAX: u64 = 4294967295; // bits as f64: 0x41ef_ffff_ffff_ffff
/// Least Exact Float (32 bits) greater-than-or-equal-to i64::MIN when rounding towards zero.
const LEF32_GEQ_I64_MIN: u64 = std::i64::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to i64::MAX when rounding towards zero.
const GEF32_LEQ_I64_MAX: u64 = 9223371487098961920; // bits as f32: 0x5eff_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to i64::MIN when rounding towards zero.
const LEF64_GEQ_I64_MIN: u64 = std::i64::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to i64::MAX when rounding towards zero.
const GEF64_LEQ_I64_MAX: u64 = 9223372036854774784; // bits as f64: 0x43df_ffff_ffff_ffff
/// Least Exact Float (32 bits) greater-than-or-equal-to u64::MIN when rounding towards zero.
const LEF32_GEQ_U64_MIN: u64 = std::u64::MIN;
/// Greatest Exact Float (32 bits) less-than-or-equal-to u64::MAX when rounding towards zero.
const GEF32_LEQ_U64_MAX: u64 = 18446742974197923840; // bits as f32: 0x5f7f_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to u64::MIN when rounding towards zero.
const LEF64_GEQ_U64_MIN: u64 = std::u64::MIN;
/// Greatest Exact Float (64 bits) less-than-or-equal-to u64::MAX when rounding towards zero.
const GEF64_LEQ_U64_MAX: u64 = 18446744073709549568; // bits as f64: 0x43ef_ffff_ffff_ffff
