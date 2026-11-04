//! Backtrace and trap information for compiled native modules.

use crate::engine::artifact::TrapReader;
use crate::engine::mapped_binary::{DebugInfo, DebugInfoSource};
use std::cmp;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, RwLock};
use wasmer_types::{
    FrameInfo, LocalFunctionIndex, ModuleInfo, SourceLoc, TrapInformation,
    entity::{BoxedSlice, EntityRef},
};
use wasmer_vm::FunctionBodyPtr;

/// Global cache used to symbolicate frames for active modules.
pub static FRAME_INFO: LazyLock<RwLock<GlobalFrameInfo>> = LazyLock::new(RwLock::default);

#[derive(Default)]
pub struct GlobalFrameInfo {
    // Each key is the highest address in a disjoint module range.
    ranges: BTreeMap<usize, ModuleInfoFrameInfo>,
}

/// Unregisters a module's frame information when dropped.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct GlobalFrameInfoRegistration {
    key: usize,
}

struct ModuleInfoFrameInfo {
    start: usize,
    functions: BTreeMap<usize, FunctionInfo>,
    module: Arc<ModuleInfo>,
    /// Base address used to translate a PC into an ELF image offset.
    image_base: usize,
    debug_info: DebugInfo,
    trap_reader: Option<TrapReader>,
}

impl ModuleInfoFrameInfo {
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
    pub fn lookup_frame_info(&self, pc: usize) -> Option<FrameInfo> {
        let module = self.module_info(pc)?;
        let func = module.function_info(pc)?;
        let func_index = module.module.func_index(func.local_index);
        let mut function_name = module.module.function_names.get(&func_index).cloned();

        // Resolve DWARF line info using offsets into the original ELF image.
        let get_line = |context: &addr2line::Context<crate::engine::mapped_binary::DwarfReader>,
                        pc: u64| {
            if let Ok(Some(location)) = context.find_location(pc)
                && let Some(line) = location.line
                && let Some(line) = line.checked_sub(1)
            {
                SourceLoc::new(line)
            } else {
                SourceLoc::default()
            }
        };

        let (func_start, instr, resolved_name) = module.debug_info.with_context(|context| {
            let Some(context) = context else {
                return (SourceLoc::default(), SourceLoc::default(), None);
            };

            let probe = (pc - module.image_base) as u64;
            let instr = get_line(context, probe);
            let func_start = get_line(context, (func.start - module.image_base) as u64);

            let resolved_name = if let Ok(mut frames) = context.find_frames(probe).skip_all_loads()
                && let Ok(Some(frame)) = frames.next()
                && let Some(function) = frame.function
                && let Ok(name) = function.raw_name()
                && name != module.module.get_function_name(func_index)
            {
                Some(name.into_owned())
            } else {
                None
            };

            (func_start, instr, resolved_name)
        });
        if let Some(resolved_name) = resolved_name {
            function_name = Some(resolved_name);
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
        let rel_pos = u32::try_from(pc - func.start).ok()?;
        module
            .trap_reader
            .as_ref()?
            .lookup(func.local_index, rel_pos)
    }

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

/// A continuous region of executable memory starting at a function entry point.
#[derive(Debug)]
#[repr(C)]
pub struct FunctionExtent {
    /// Entry point of the function.
    pub ptr: FunctionBodyPtr,
    /// Length in bytes.
    pub length: usize,
}

/// Registers a compiled module's frame information until the result is dropped.
/// Returns `None` for modules without functions.
pub fn register(
    module: Arc<ModuleInfo>,
    finished_functions: &BoxedSlice<LocalFunctionIndex, FunctionExtent>,
    image_base: usize,
    elf_data: Option<Arc<[u8]>>,
) -> Option<GlobalFrameInfoRegistration> {
    register_with_source(
        module,
        finished_functions,
        image_base,
        elf_data.map(DebugInfoSource::Bytes),
    )
}

pub(crate) fn register_with_source(
    module: Arc<ModuleInfo>,
    finished_functions: &BoxedSlice<LocalFunctionIndex, FunctionExtent>,
    image_base: usize,
    elf_data: Option<DebugInfoSource>,
) -> Option<GlobalFrameInfoRegistration> {
    let trap_reader = elf_data
        .clone()
        .map(|source| TrapReader::new(source, image_base));
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
        // End is the last byte of the function code.
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
    // Module code ranges must not overlap.
    if let Some((_, prev)) = info.ranges.range(max..).next() {
        assert!(prev.start > max);
    }
    if let Some((prev_end, _)) = info.ranges.range(..=min).next_back() {
        assert!(*prev_end < min);
    }

    let prev = info.ranges.insert(
        max,
        ModuleInfoFrameInfo {
            start: min,
            functions,
            module,
            image_base,
            debug_info: DebugInfo::new(elf_data),
            trap_reader,
        },
    );
    assert!(prev.is_none());
    Some(GlobalFrameInfoRegistration { key: max })
}
