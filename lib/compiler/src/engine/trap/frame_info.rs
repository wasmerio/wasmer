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

use addr2line::Loader;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, RwLock};
use wasmer_types::lib::std::{cmp, ops::Deref};
use wasmer_types::{
    FrameInfo, LocalFunctionIndex, ModuleInfo, SourceLoc, TrapInformation,
    entity::{BoxedSlice, EntityRef, PrimaryMap},
};
use wasmer_vm::FunctionBodyPtr;

/// This is a global cache of backtrace frame information for all active
///
/// This global cache is used during `Trap` creation to symbolicate frames.
/// This is populated on module compilation, and it is cleared out whenever
/// all references to a module are dropped.
pub static FRAME_INFO: LazyLock<RwLock<GlobalFrameInfo>> = LazyLock::new(RwLock::default);

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
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct GlobalFrameInfoRegistration {
    /// The key that will be removed from the global `ranges` map when this is
    /// dropped.
    key: usize,
}

struct ModuleInfoFrameInfo {
    start: usize,
    image_base: usize,
    functions: BTreeMap<usize, FunctionInfo>,
    module: Arc<ModuleInfo>,
    trap_infos: BoxedSlice<LocalFunctionIndex, Vec<TrapInformation>>,
    debug_info: Arc<Mutex<addr2line::Loader>>,
}

impl ModuleInfoFrameInfo {
    /// Gets a function given a pc
    fn function_info(&self, pc: usize) -> Option<&FunctionInfo> {
        let (end, func) = self.functions.range(pc..).next()?;
        if func.start <= pc && pc <= *end {
            Some(func)
        } else {
            None
        }
    }

    fn trap_info(&self, func: &FunctionInfo, pc: usize) -> Option<TrapInformation> {
        let rel_pos = (pc - func.start) as u32;
        if let Some(traps) = self.trap_infos.get(func.local_index)
            && !traps.is_empty()
        {
            let idx = traps
                .binary_search_by_key(&rel_pos, |info| info.code_offset)
                .ok()?;
            return Some(traps[idx]);
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
        let func_index = module.module.func_index(func.local_index);
        let mut function_name = module.module.function_names.get(&func_index).cloned();

        let get_line = |debug_info: &MutexGuard<Loader>, pc| {
            if let Ok(Some(location)) = debug_info.find_location(pc)
                && let Some(line) = location.line
                && let Some(line) = line.checked_sub(1)
            {
                SourceLoc::new(line)
            } else {
                SourceLoc::default()
            }
        };

        let mut instr = SourceLoc::default();
        let mut func_start = SourceLoc::default();
        if let Ok(debug_info) = module.debug_info.lock() {
            let probe = (pc - module.image_base) as u64;

            instr = get_line(&debug_info, probe);
            func_start = get_line(&debug_info, (func.start - module.image_base) as u64);

            if let Ok(mut frames) = debug_info.find_frames(probe)
                && let Ok(Some(frame)) = frames.next()
                && let Some(function) = frame.function
                && let Ok(name) = function.raw_name()
                // TODO: add constant
                && name != "<unnamed>"
            {
                function_name = Some(name.into_owned());
            }
        }
        Some(FrameInfo::new(
            module.module.name(),
            func_index.index() as u32,
            function_name,
            func_start,
            instr,
        ))
    }

    /// Fetches trap information about a program counter in a backtrace.
    pub fn lookup_trap_info(&self, pc: usize) -> Option<TrapInformation> {
        let module = self.module_info(pc)?;
        let func = module.function_info(pc)?;
        module.trap_info(func, pc)
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

/// The variant of the trap information which can be an owned type
#[derive(Debug)]
pub enum VecTrapInformationVariant<'a> {
    Ref(&'a Vec<TrapInformation>),
    Owned(Vec<TrapInformation>),
}

// We need to implement it for the `Deref` in `wasmer_types` to support both `core` and `std`.
impl Deref for VecTrapInformationVariant<'_> {
    type Target = [TrapInformation];

    fn deref(&self) -> &Self::Target {
        match self {
            VecTrapInformationVariant::Ref(traps) => traps,
            VecTrapInformationVariant::Owned(traps) => traps,
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
    trap_infos: BoxedSlice<LocalFunctionIndex, Vec<TrapInformation>>,
    image_base: usize,
    #[cfg(target_os = "linux")] debug_info: Arc<Mutex<addr2line::Loader>>,
) -> Option<GlobalFrameInfoRegistration> {
    let mut min = usize::MAX;
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
            image_base,
            functions,
            module,
            trap_infos,
            #[cfg(target_os = "linux")]
            debug_info,
        },
    );
    assert!(prev.is_none());
    Some(GlobalFrameInfoRegistration { key: max })
}
