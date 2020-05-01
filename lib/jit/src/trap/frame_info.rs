//! This module is used for having backtraces in the Wasm runtime.
//! Once the Compiler has compiled the Module, and we have a set of
//! compiled functions (addresses and function index) and a module,
//! then we can use this to set a backtrace for that module.
//!
//! # Example
//! ```ignore
//! use wasmer_runtime::{Module, FRAME_INFO};
//!
//! let module: Module = ...;
//! FRAME_INFO.register(module, compiled_functions);
//! ```
use crate::CompiledModule;
use std::cmp;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::LocalFuncIndex;
use wasmer_compiler::{FunctionAddressMap, SourceLoc, TrapInformation};
use wasmer_runtime::Module;

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
    ranges: BTreeMap<usize, ModuleFrameInfo>,
}

/// An RAII structure used to unregister a module's frame information when the
/// module is destroyed.
pub struct GlobalFrameInfoRegistration {
    /// The key that will be removed from the global `ranges` map when this is
    /// dropped.
    key: usize,
}

struct FunctionDebugInfo {
    traps: Vec<TrapInformation>,
    instr_map: FunctionAddressMap,
}

struct ModuleDebugInfo {
    functions: PrimaryMap<LocalFuncIndex, FunctionDebugInfo>,
}

struct ModuleFrameInfo {
    start: usize,
    functions: BTreeMap<usize, FunctionInfo>,
    module: Arc<Module>,
    debug: ModuleDebugInfo,
}

impl ModuleFrameInfo {
    fn instr_map(&self, local_index: LocalFuncIndex) -> &FunctionAddressMap {
        &self.debug.functions.get(local_index).unwrap().instr_map
    }
    fn traps(&self, local_index: LocalFuncIndex) -> &Vec<TrapInformation> {
        &self.debug.functions.get(local_index).unwrap().traps
    }
}

impl ModuleFrameInfo {
    /// Gets a function given a pc
    fn function(&self, pc: usize) -> Option<&FunctionInfo> {
        let (end, func) = self.functions.range(pc..).next()?;
        if pc < func.start || *end < pc {
            return None;
        }
        Some(func)
    }
}

struct FunctionInfo {
    start: usize,
    local_index: LocalFuncIndex,
}

impl GlobalFrameInfo {
    /// Fetches frame information about a program counter in a backtrace.
    ///
    /// Returns an object if this `pc` is known to some previously registered
    /// module, or returns `None` if no information can be found.
    pub fn lookup_frame_info(&self, pc: usize) -> Option<FrameInfo> {
        let module = self.module(pc)?;
        let func = module.function(pc)?;

        // Use our relative position from the start of the function to find the
        // machine instruction that corresponds to `pc`, which then allows us to
        // map that to a wasm original source location.
        let rel_pos = pc - func.start;
        let instr_map = module.instr_map(func.local_index);
        let pos = match instr_map
            .instructions
            .binary_search_by_key(&rel_pos, |map| map.code_offset)
        {
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
                let instr = &instr_map.instructions[n - 1];
                if instr.code_offset <= rel_pos && rel_pos < instr.code_offset + instr.code_len {
                    Some(n - 1)
                } else {
                    None
                }
            }
        };

        // In debug mode for now assert that we found a mapping for `pc` within
        // the function, because otherwise something is buggy along the way and
        // not accounting for all the instructions. This isn't super critical
        // though so we can omit this check in release mode.
        debug_assert!(pos.is_some(), "failed to find instruction for {:x}", pc);

        let instr = match pos {
            Some(pos) => instr_map.instructions[pos].srcloc,
            None => instr_map.start_srcloc,
        };
        let func_index = module.module.func_index(func.local_index);
        Some(FrameInfo {
            module_name: module.module.name(),
            func_index: func_index.index() as u32,
            func_name: module.module.func_names.get(&func_index).cloned(),
            instr,
            func_start: instr_map.start_srcloc,
        })
    }

    /// Fetches trap information about a program counter in a backtrace.
    pub fn lookup_trap_info(&self, pc: usize) -> Option<&TrapInformation> {
        let module = self.module(pc)?;
        let func = module.function(pc)?;
        let traps = module.traps(func.local_index);
        let idx = traps
            .binary_search_by_key(&((pc - func.start) as u32), |info| info.code_offset)
            .ok()?;
        Some(&traps[idx])
    }

    /// Gets a module given a pc
    fn module(&self, pc: usize) -> Option<&ModuleFrameInfo> {
        let (end, module_info) = self.ranges.range(pc..).next()?;
        if pc < module_info.start || *end < pc {
            return None;
        }
        Some(module_info)
    }

    /// Gets a module given a pc
    fn module_mut(&mut self, pc: usize) -> Option<&mut ModuleFrameInfo> {
        let (end, module_info) = self.ranges.range_mut(pc..).next()?;
        if pc < module_info.start || *end < pc {
            return None;
        }
        Some(module_info)
    }
}

impl Drop for GlobalFrameInfoRegistration {
    fn drop(&mut self) {
        if let Ok(mut info) = FRAME_INFO.write() {
            info.ranges.remove(&self.key);
        }
    }
}

/// Registers a new compiled module's frame information.
///
/// This function will register the `names` information for all of the
/// compiled functions within `module`. If the `module` has no functions
/// then `None` will be returned. Otherwise the returned object, when
/// dropped, will be used to unregister all name information from this map.
pub fn register(module: &CompiledModule) -> Option<GlobalFrameInfoRegistration> {
    let mut min = usize::max_value();
    let mut max = 0;
    let mut functions = BTreeMap::new();
    for (i, allocated) in module.finished_functions().iter() {
        let (start, end) = unsafe {
            let ptr = (**allocated).as_ptr();
            let len = (**allocated).len();
            (ptr as usize, ptr as usize + len)
        };
        min = cmp::min(min, start);
        max = cmp::max(max, end);
        let func = FunctionInfo {
            start,
            local_index: i,
        };
        assert!(functions.insert(end, func).is_none());
    }
    if functions.len() == 0 {
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

    let debug_functions = module
        .traps()
        .values()
        .zip(module.address_transform().values())
        .map(|(traps, instrs)| FunctionDebugInfo {
            traps: traps.to_vec(),
            instr_map: (*instrs).clone(),
        })
        .collect::<PrimaryMap<LocalFuncIndex, _>>();

    let debug = ModuleDebugInfo {
        functions: debug_functions,
    };

    // ... then insert our range and assert nothing was there previously
    let prev = info.ranges.insert(
        max,
        ModuleFrameInfo {
            start: min,
            functions,
            module: module.module().clone(),
            debug,
        },
    );
    assert!(prev.is_none());
    Some(GlobalFrameInfoRegistration { key: max })
}

/// Description of a frame in a backtrace for a [`Trap`].
///
/// Whenever a WebAssembly trap occurs an instance of [`Trap`] is created. Each
/// [`Trap`] has a backtrace of the WebAssembly frames that led to the trap, and
/// each frame is described by this structure.
///
/// [`Trap`]: crate::Trap
#[derive(Debug)]
pub struct FrameInfo {
    module_name: String,
    func_index: u32,
    func_name: Option<String>,
    func_start: SourceLoc,
    instr: SourceLoc,
}

impl FrameInfo {
    /// Returns the WebAssembly function index for this frame.
    ///
    /// This function index is the index in the function index space of the
    /// WebAssembly module that this frame comes from.
    pub fn func_index(&self) -> u32 {
        self.func_index
    }

    /// Returns the identifer of the module that this frame is for.
    ///
    /// Module identifiers are present in the `name` section of a WebAssembly
    /// binary, but this may not return the exact item in the `name` section.
    /// Module names can be overwritten at construction time or perhaps inferred
    /// from file names. The primary purpose of this function is to assist in
    /// debugging and therefore may be tweaked over time.
    ///
    /// This function returns `None` when no name can be found or inferred.
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    /// Returns a descriptive name of the function for this frame, if one is
    /// available.
    ///
    /// The name of this function may come from the `name` section of the
    /// WebAssembly binary, or wasmer may try to infer a better name for it if
    /// not available, for example the name of the export if it's exported.
    ///
    /// This return value is primarily used for debugging and human-readable
    /// purposes for things like traps. Note that the exact return value may be
    /// tweaked over time here and isn't guaranteed to be something in
    /// particular about a wasm module due to its primary purpose of assisting
    /// in debugging.
    ///
    /// This function returns `None` when no name could be inferred.
    pub fn func_name(&self) -> Option<&str> {
        self.func_name.as_deref()
    }

    /// Returns the offset within the original wasm module this frame's program
    /// counter was at.
    ///
    /// The offset here is the offset from the beginning of the original wasm
    /// module to the instruction that this frame points to.
    pub fn module_offset(&self) -> usize {
        self.instr.bits() as usize
    }

    /// Returns the offset from the original wasm module's function to this
    /// frame's program counter.
    ///
    /// The offset here is the offset from the beginning of the defining
    /// function of this frame (within the wasm module) to the instruction this
    /// frame points to.
    pub fn func_offset(&self) -> usize {
        (self.instr.bits() - self.func_start.bits()) as usize
    }
}
