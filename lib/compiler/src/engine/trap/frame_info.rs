//! This module is used for having backtraces in the Wasm runtime.
//! Once the Compiler has compiled the ModuleInfo, and we have a set of
//! compiled functions (addresses and function index) and a module,
//! then we can use this to set a backtrace for that module.
//!
//! # Example
//! ```ignore
//! use wasmer_vm::{FRAME_INFO};
//! use wasmer_types::ModuleInfo;
//!
//! let module: ModuleInfo = ...;
//! FRAME_INFO.register(module, compiled_functions);
//! ```
use core::ops::Deref;
use rkyv::vec::ArchivedVec;
use std::cmp;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use wasmer_types::compilation::address_map::{
    ArchivedFunctionAddressMap, ArchivedInstructionAddressMap,
};
use wasmer_types::compilation::function::ArchivedCompiledFunctionFrameInfo;
use wasmer_types::entity::{BoxedSlice, EntityRef, PrimaryMap};
use wasmer_types::{
    CompiledFunctionFrameInfo, FrameInfo, FunctionAddressMap, InstructionAddressMap,
    LocalFunctionIndex, ModuleInfo, SourceLoc, TrapInformation,
};
use wasmer_vm::FunctionBodyPtr;

use crate::ArtifactBuildFromArchive;

lazy_static::lazy_static! {
    /// This is a global cache of backtrace frame information for all active
    ///
    /// This global cache is used during `Trap` creation to symbolicate frames.
    /// This is populated on module compilation, and it is cleared out whenever
    /// all references to a module are dropped.
    pub static ref FRAME_INFO: RwLock<GlobalFrameInfo> = Default::default();
}

#[derive(Default)]
pub struct GlobalFrameInfo {
    /// An internal map that keeps track of backtrace frame information for
    /// each module.
    ///
    /// This map is morally a map of ranges to a map of information for that
    /// module. Each module is expected to reside in a disjoint section of
    /// contiguous memory. No modules can overlap.
    ///
    /// The key of this map is the highest address in the module and the value
    /// is the module's information, which also contains the start address.
    ranges: BTreeMap<usize, ModuleInfoFrameInfo>,
}

/// An RAII structure used to unregister a module's frame information when the
/// module is destroyed.
pub struct GlobalFrameInfoRegistration {
    /// The key that will be removed from the global `ranges` map when this is
    /// dropped.
    key: usize,
}

#[derive(Debug)]
struct ModuleInfoFrameInfo {
    start: usize,
    functions: BTreeMap<usize, FunctionInfo>,
    module: Arc<ModuleInfo>,
    frame_infos: FrameInfosVariant,
}

impl ModuleInfoFrameInfo {
    fn function_debug_info(
        &self,
        local_index: LocalFunctionIndex,
    ) -> CompiledFunctionFrameInfoVariant {
        self.frame_infos.get(local_index).unwrap()
    }

    /// Gets a function given a pc
    fn function_info(&self, pc: usize) -> Option<&FunctionInfo> {
        let (end, func) = self.functions.range(pc..).next()?;
        if func.start <= pc && pc <= *end {
            Some(func)
        } else {
            None
        }
    }
}

#[derive(Debug)]
struct FunctionInfo {
    start: usize,
    local_index: LocalFunctionIndex,
}

impl GlobalFrameInfo {
    /// Fetches frame information about a program counter in a backtrace.
    ///
    /// Returns an object if this `pc` is known to some previously registered
    /// module, or returns `None` if no information can be found.
    pub fn lookup_frame_info(&self, pc: usize) -> Option<FrameInfo> {
        let module = self.module_info(pc)?;
        let func = module.function_info(pc)?;

        // Use our relative position from the start of the function to find the
        // machine instruction that corresponds to `pc`, which then allows us to
        // map that to a wasm original source location.
        let rel_pos = pc - func.start;
        let debug_info = module.function_debug_info(func.local_index);
        let instr_map = debug_info.address_map();
        let pos = match instr_map.instructions().code_offset_by_key(rel_pos) {
            // Exact hit!
            Ok(pos) => Some(pos),

            // This *would* be at the first slot in the array, so no
            // instructions cover `pc`.
            Err(0) => None,

            // This would be at the `nth` slot, so check `n-1` to see if we're
            // part of that instruction. This happens due to the minus one when
            // this function is called form trap symbolication, where we don't
            // always get called with a `pc` that's an exact instruction
            // boundary.
            Err(n) => {
                let instr = &instr_map.instructions().get(n - 1);
                if instr.code_offset <= rel_pos && rel_pos < instr.code_offset + instr.code_len {
                    Some(n - 1)
                } else {
                    None
                }
            }
        };

        let instr = match pos {
            Some(pos) => instr_map.instructions().get(pos).srcloc,
            // Some compilers don't emit yet the full trap information for each of
            // the instructions (such as LLVM).
            // In case no specific instruction is found, we return by default the
            // start offset of the function.
            None => instr_map.start_srcloc(),
        };
        let func_index = module.module.func_index(func.local_index);
        Some(FrameInfo::new(
            module.module.name(),
            func_index.index() as u32,
            module.module.function_names.get(&func_index).cloned(),
            instr_map.start_srcloc(),
            instr,
        ))
    }

    /// Fetches trap information about a program counter in a backtrace.
    pub fn lookup_trap_info(&self, pc: usize) -> Option<TrapInformation> {
        let module = self.module_info(pc)?;
        let func = module.function_info(pc)?;
        let debug_info = module.function_debug_info(func.local_index);
        let traps = debug_info.traps();
        let idx = traps
            .binary_search_by_key(&((pc - func.start) as u32), |info| info.code_offset)
            .ok()?;
        Some(traps[idx])
    }

    /// Gets a module given a pc
    fn module_info(&self, pc: usize) -> Option<&ModuleInfoFrameInfo> {
        let (end, module_info) = self.ranges.range(pc..).next()?;
        if module_info.start <= pc && pc <= *end {
            Some(module_info)
        } else {
            None
        }
    }
}

impl Drop for GlobalFrameInfoRegistration {
    fn drop(&mut self) {
        if let Ok(mut info) = FRAME_INFO.write() {
            info.ranges.remove(&self.key);
        }
    }
}

/// Represents a continuous region of executable memory starting with a function
/// entry point.
#[derive(Debug)]
#[repr(C)]
pub struct FunctionExtent {
    /// Entry point for normal entry of the function. All addresses in the
    /// function lie after this address.
    pub ptr: FunctionBodyPtr,
    /// Length in bytes.
    pub length: usize,
}

/// The variant of the frame information which can be an owned type
/// or the explicit framed map
#[derive(Debug)]
pub enum FrameInfosVariant {
    /// Owned frame infos
    Owned(PrimaryMap<LocalFunctionIndex, CompiledFunctionFrameInfo>),
    /// Archived frame infos
    Archived(ArtifactBuildFromArchive),
}

impl FrameInfosVariant {
    /// Gets the frame info for a given local function index
    pub fn get(&self, index: LocalFunctionIndex) -> Option<CompiledFunctionFrameInfoVariant> {
        match self {
            Self::Owned(map) => map.get(index).map(CompiledFunctionFrameInfoVariant::Ref),
            Self::Archived(archive) => archive
                .get_frame_info_ref()
                .get(index)
                .map(CompiledFunctionFrameInfoVariant::Archived),
        }
    }
}

/// The variant of the compiled function frame info which can be an owned type
#[derive(Debug)]
pub enum CompiledFunctionFrameInfoVariant<'a> {
    /// A reference to the frame info
    Ref(&'a CompiledFunctionFrameInfo),
    /// An archived frame info
    Archived(&'a ArchivedCompiledFunctionFrameInfo),
}

impl CompiledFunctionFrameInfoVariant<'_> {
    /// Gets the address map for the frame info
    pub fn address_map(&self) -> FunctionAddressMapVariant<'_> {
        match self {
            CompiledFunctionFrameInfoVariant::Ref(info) => {
                FunctionAddressMapVariant::Ref(&info.address_map)
            }
            CompiledFunctionFrameInfoVariant::Archived(info) => {
                FunctionAddressMapVariant::Archived(&info.address_map)
            }
        }
    }

    /// Gets the traps for the frame info
    pub fn traps(&self) -> VecTrapInformationVariant {
        match self {
            CompiledFunctionFrameInfoVariant::Ref(info) => {
                VecTrapInformationVariant::Ref(&info.traps)
            }
            CompiledFunctionFrameInfoVariant::Archived(info) => {
                VecTrapInformationVariant::Archived(&info.traps)
            }
        }
    }
}

/// The variant of the trap information which can be an owned type
#[derive(Debug)]
pub enum VecTrapInformationVariant<'a> {
    Ref(&'a Vec<TrapInformation>),
    Archived(&'a ArchivedVec<TrapInformation>),
}

impl Deref for VecTrapInformationVariant<'_> {
    type Target = [TrapInformation];

    fn deref(&self) -> &Self::Target {
        match self {
            VecTrapInformationVariant::Ref(traps) => traps,
            VecTrapInformationVariant::Archived(traps) => traps,
        }
    }
}

#[derive(Debug)]
pub enum FunctionAddressMapVariant<'a> {
    Ref(&'a FunctionAddressMap),
    Archived(&'a ArchivedFunctionAddressMap),
}

impl FunctionAddressMapVariant<'_> {
    pub fn instructions(&self) -> FunctionAddressMapInstructionVariant {
        match self {
            FunctionAddressMapVariant::Ref(map) => {
                FunctionAddressMapInstructionVariant::Owned(&map.instructions)
            }
            FunctionAddressMapVariant::Archived(map) => {
                FunctionAddressMapInstructionVariant::Archived(&map.instructions)
            }
        }
    }

    pub fn start_srcloc(&self) -> SourceLoc {
        match self {
            FunctionAddressMapVariant::Ref(map) => map.start_srcloc,
            FunctionAddressMapVariant::Archived(map) => map.start_srcloc,
        }
    }

    pub fn end_srcloc(&self) -> SourceLoc {
        match self {
            FunctionAddressMapVariant::Ref(map) => map.end_srcloc,
            FunctionAddressMapVariant::Archived(map) => map.end_srcloc,
        }
    }

    pub fn body_offset(&self) -> usize {
        match self {
            FunctionAddressMapVariant::Ref(map) => map.body_offset,
            FunctionAddressMapVariant::Archived(map) => map.body_offset as usize,
        }
    }

    pub fn body_len(&self) -> usize {
        match self {
            FunctionAddressMapVariant::Ref(map) => map.body_len,
            FunctionAddressMapVariant::Archived(map) => map.body_len as usize,
        }
    }
}

#[derive(Debug)]
pub enum FunctionAddressMapInstructionVariant<'a> {
    Owned(&'a Vec<InstructionAddressMap>),
    Archived(&'a ArchivedVec<ArchivedInstructionAddressMap>),
}

impl FunctionAddressMapInstructionVariant<'_> {
    pub fn code_offset_by_key(&self, key: usize) -> Result<usize, usize> {
        match self {
            FunctionAddressMapInstructionVariant::Owned(instructions) => {
                instructions.binary_search_by_key(&key, |map| map.code_offset)
            }
            FunctionAddressMapInstructionVariant::Archived(instructions) => {
                instructions.binary_search_by_key(&key, |map| map.code_offset as usize)
            }
        }
    }

    pub fn get(&self, index: usize) -> InstructionAddressMap {
        match self {
            FunctionAddressMapInstructionVariant::Owned(instructions) => instructions[index],
            FunctionAddressMapInstructionVariant::Archived(instructions) => instructions
                .get(index)
                .map(|map| InstructionAddressMap {
                    srcloc: map.srcloc,
                    code_offset: map.code_offset as usize,
                    code_len: map.code_len as usize,
                })
                .unwrap(),
        }
    }
}

/// Registers a new compiled module's frame information.
///
/// This function will register the `names` information for all of the
/// compiled functions within `module`. If the `module` has no functions
/// then `None` will be returned. Otherwise the returned object, when
/// dropped, will be used to unregister all name information from this map.
pub fn register(
    module: Arc<ModuleInfo>,
    finished_functions: &BoxedSlice<LocalFunctionIndex, FunctionExtent>,
    frame_infos: FrameInfosVariant,
) -> Option<GlobalFrameInfoRegistration> {
    let mut min = usize::max_value();
    let mut max = 0;
    let mut functions = BTreeMap::new();
    for (
        i,
        FunctionExtent {
            ptr: start,
            length: len,
        },
    ) in finished_functions.iter()
    {
        let start = **start as usize;
        // end is "last byte" of the function code
        let end = start + len - 1;
        min = cmp::min(min, start);
        max = cmp::max(max, end);
        let func = FunctionInfo {
            start,
            local_index: i,
        };
        assert!(functions.insert(end, func).is_none());
    }
    if functions.is_empty() {
        return None;
    }

    let mut info = FRAME_INFO.write().unwrap();
    // First up assert that our chunk of jit functions doesn't collide with
    // any other known chunks of jit functions...
    if let Some((_, prev)) = info.ranges.range(max..).next() {
        assert!(prev.start > max);
    }
    if let Some((prev_end, _)) = info.ranges.range(..=min).next_back() {
        assert!(*prev_end < min);
    }

    // ... then insert our range and assert nothing was there previously
    let prev = info.ranges.insert(
        max,
        ModuleInfoFrameInfo {
            start: min,
            functions,
            module,
            frame_infos,
        },
    );
    assert!(prev.is_none());
    Some(GlobalFrameInfoRegistration { key: max })
}
