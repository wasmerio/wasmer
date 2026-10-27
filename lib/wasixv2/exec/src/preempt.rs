//! Preemption points. Wasm has no timer interrupt, so the loader instruments every module: each
//! function entry and loop header counts down a budget kept in a global of the instance, and when
//! it runs out the guest calls the hook import, which runs the return-to-guest path (signals,
//! exit) and hands out a new budget. A busy guest thus sees signals, `SIGKILL` and the exit of
//! its thread group within a bounded number of steps, as DESIGN's "async signals only at
//! interruption checks" describes; between checks it runs undisturbed.
//!
//! The same pass turns wasm's `memory.atomic.wait32`, `wait64` and `notify` on a shared 32-bit
//! memory into calls of imports that wait and wake on the kernel's futexes ([`crate::atomics`]).
//! The engine would block the host thread in the instruction, where neither a signal nor the
//! group's exit reaches the guest, and only another instruction could wake it.
//!
//! The rewrite only adds: four types, four function imports after the others (so defined
//! functions move up by four indices), one global, the check, and a constant with each atomic's
//! offset before the call that replaces it. The start function is left alone, as it runs during
//! instantiation, before the instance can take signals or has threads to wait for.

use std::{borrow::Cow, convert::Infallible};

use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, EntityType, Function, GlobalSection, GlobalType,
    ImportSection, Instruction, Module, SectionId, TypeSection, ValType,
    reencode::{Error, Reencode, utils},
};
use wasmparser::{
    FunctionBody, GlobalSectionReader, ImportSectionReader, Parser, RecGroup, TypeRef,
    TypeSectionReader,
};

/// The imports the instrumentation adds; not part of the guest ABI.
pub(crate) const HOOK_MODULE: &str = "wasixv2-exec";
pub(crate) const HOOK_NAME: &str = "preempt";
/// `(addr, expected, timeout, offset) -> result`, as the instructions with their offset last.
pub(crate) const WAIT32_NAME: &str = "memory-atomic-wait32";
pub(crate) const WAIT64_NAME: &str = "memory-atomic-wait64";
/// `(addr, count, offset) -> woken`.
pub(crate) const NOTIFY_NAME: &str = "memory-atomic-notify";

/// The added imports in order, each with its parameters; all return an `i32`.
const HOOKS: [(&str, &[ValType]); 4] = [
    (HOOK_NAME, &[]),
    (
        WAIT32_NAME,
        &[ValType::I32, ValType::I32, ValType::I64, ValType::I32],
    ),
    (
        WAIT64_NAME,
        &[ValType::I32, ValType::I64, ValType::I64, ValType::I32],
    ),
    (NOTIFY_NAME, &[ValType::I32, ValType::I32, ValType::I32]),
];

/// Checks between two calls of the hook. A check costs a few instructions, a call of the hook
/// with nothing to do about a microsecond, so this keeps the overhead of the calls well below
/// one percent while a busy loop still notices a signal within a millisecond or so.
pub(crate) const BUDGET: i32 = 1 << 16;

/// `module` with preemption points, or as it is when it cannot be parsed, so that compiling it
/// reports what is wrong with it.
pub(crate) fn instrument(module: &[u8]) -> Cow<'_, [u8]> {
    let mut out = Module::new();
    let mut points = Points::default();
    match points.parse_core_module(&mut out, Parser::new(0), module) {
        Ok(()) => Cow::Owned(out.finish()),
        Err(err) => {
            tracing::debug!(%err, "cannot add preemption points to the module");
            Cow::Borrowed(module)
        }
    }
}

#[derive(Default)]
struct Points {
    /// Types seen so far, and then the first of the hooks' types.
    types: u32,
    hook_type: Option<u32>,
    /// Function imports: the first hook's index, and how far defined functions move.
    imported_funcs: Option<u32>,
    /// Whether memory 0 is shared and 32-bit, so its atomics wait on the kernel's futexes; set
    /// by the first memory type, which is memory 0's.
    futex_memory: Option<bool>,
    imported_globals: u32,
    budget: Option<u32>,
    start: Option<u32>,
    /// Bodies seen so far.
    bodies: u32,
}

/// The order of the non-custom sections.
const ORDER: [SectionId; 13] = [
    SectionId::Type,
    SectionId::Import,
    SectionId::Function,
    SectionId::Table,
    SectionId::Memory,
    SectionId::Tag,
    SectionId::Global,
    SectionId::Export,
    SectionId::Start,
    SectionId::Element,
    SectionId::DataCount,
    SectionId::Code,
    SectionId::Data,
];

/// Whether a section that comes before `before` (or at the end, for `None`) belongs after
/// `section`.
fn past(section: SectionId, before: Option<SectionId>) -> bool {
    let rank = |id: SectionId| ORDER.iter().position(|&s| s as u8 == id as u8);
    match before {
        None => true,
        Some(before) => rank(before) > rank(section),
    }
}

impl Points {
    fn hook_type(&self) -> u32 {
        self.hook_type
            .expect("the type section comes before the imports")
    }

    /// The preemption hook's index; the atomics' follow it in [`HOOKS`]' order.
    fn hook(&self) -> u32 {
        self.imported_funcs
            .expect("the import section comes before the code")
    }

    fn add_hook_type(&mut self, types: &mut TypeSection) {
        self.hook_type = Some(self.types);
        for (_, params) in HOOKS {
            types.ty().function(params.iter().copied(), [ValType::I32]);
        }
    }

    fn add_hook(&mut self, imports: &mut ImportSection) {
        for (at, (name, _)) in (0..).zip(HOOKS) {
            imports.import(
                HOOK_MODULE,
                name,
                EntityType::Function(self.hook_type() + at),
            );
        }
    }

    /// The call that replaces `instruction`, an atomic wait or notify on memory 0: its offset,
    /// then the hook. `None` for every other instruction, and when memory 0 is not a shared
    /// 32-bit memory, where the engine's own instruction is right (it traps, or wakes nobody).
    fn atomic_call(&self, instruction: &Instruction<'_>) -> Option<[Instruction<'static>; 2]> {
        let (at, memarg) = match instruction {
            Instruction::MemoryAtomicWait32(memarg) => (1, memarg),
            Instruction::MemoryAtomicWait64(memarg) => (2, memarg),
            Instruction::MemoryAtomicNotify(memarg) => (3, memarg),
            _ => return None,
        };
        let offset = u32::try_from(memarg.offset).ok()?;
        (memarg.memory_index == 0 && self.futex_memory == Some(true)).then(|| {
            [
                Instruction::I32Const(offset as i32),
                Instruction::Call(self.hook() + at),
            ]
        })
    }

    fn add_budget(&mut self, globals: &mut GlobalSection, index: u32) {
        let ty = GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        };
        globals.global(ty, &ConstExpr::i32_const(BUDGET));
        self.budget = Some(index);
    }

    /// `if budget { budget -= 1 } else { budget = hook() }`.
    fn check(&self, f: &mut Function) {
        let budget = self.budget.expect("the globals come before the code");
        f.instruction(&Instruction::GlobalGet(budget));
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::GlobalGet(budget));
        f.instruction(&Instruction::I32Const(1));
        f.instruction(&Instruction::I32Sub);
        f.instruction(&Instruction::GlobalSet(budget));
        f.instruction(&Instruction::Else);
        f.instruction(&Instruction::Call(self.hook()));
        f.instruction(&Instruction::GlobalSet(budget));
        f.instruction(&Instruction::End);
    }
}

impl Reencode for Points {
    type Error = Infallible;

    fn function_index(&mut self, func: u32) -> Result<u32, Error> {
        let imported = self.hook();
        Ok(if func >= imported {
            func + HOOKS.len() as u32
        } else {
            func
        })
    }

    fn start_section(&mut self, start: u32) -> Result<u32, Error> {
        self.start = Some(start);
        self.function_index(start)
    }

    fn parse_type_section(
        &mut self,
        types: &mut TypeSection,
        section: TypeSectionReader<'_>,
    ) -> Result<(), Error> {
        utils::parse_type_section(self, types, section)?;
        self.add_hook_type(types);
        Ok(())
    }

    fn memory_type(
        &mut self,
        memory_ty: wasmparser::MemoryType,
    ) -> Result<wasm_encoder::MemoryType, Error> {
        self.futex_memory
            .get_or_insert(memory_ty.shared && !memory_ty.memory64);
        Ok(utils::memory_type(self, memory_ty))
    }

    fn parse_recursive_type_group(
        &mut self,
        encoder: wasm_encoder::CoreTypeEncoder,
        rec_group: RecGroup,
    ) -> Result<(), Error> {
        self.types += rec_group.types().len() as u32;
        utils::parse_recursive_type_group(self, encoder, rec_group)
    }

    fn parse_import_section(
        &mut self,
        imports: &mut ImportSection,
        section: ImportSectionReader<'_>,
    ) -> Result<(), Error> {
        let (mut funcs, mut globals) = (0, 0);
        for import in section.clone().into_imports() {
            match import?.ty {
                TypeRef::Func(_) | TypeRef::FuncExact(_) => funcs += 1,
                TypeRef::Global(_) => globals += 1,
                _ => {}
            }
        }
        self.imported_funcs = Some(funcs);
        self.imported_globals = globals;
        utils::parse_import_section(self, imports, section)?;
        self.add_hook(imports);
        Ok(())
    }

    fn parse_global_section(
        &mut self,
        globals: &mut GlobalSection,
        section: GlobalSectionReader<'_>,
    ) -> Result<(), Error> {
        let index = self.imported_globals + section.count();
        utils::parse_global_section(self, globals, section)?;
        self.add_budget(globals, index);
        Ok(())
    }

    fn parse_function_body(
        &mut self,
        code: &mut CodeSection,
        func: FunctionBody<'_>,
    ) -> Result<(), Error> {
        let index = self.hook() + self.bodies;
        self.bodies += 1;
        let checked = self.start != Some(index);
        let mut f = self.new_function_with_parsed_locals(&func)?;
        if checked {
            self.check(&mut f);
        }
        let mut reader = func.get_operators_reader()?;
        while !reader.eof() {
            let instruction = self.parse_instruction(&mut reader)?;
            let header = matches!(instruction, Instruction::Loop(_));
            match self.atomic_call(&instruction).filter(|_| checked) {
                Some([offset, call]) => {
                    f.instruction(&offset);
                    f.instruction(&call);
                }
                None => {
                    f.instruction(&instruction);
                }
            }
            if header && checked {
                self.check(&mut f);
            }
        }
        code.function(&f);
        Ok(())
    }

    /// Adds the sections the module lacks: the hook needs a type and an import, the budget a
    /// global.
    fn intersperse_section_hook(
        &mut self,
        module: &mut Module,
        _after: Option<SectionId>,
        before: Option<SectionId>,
    ) -> Result<(), Error> {
        if self.hook_type.is_none() && past(SectionId::Type, before) {
            let mut types = TypeSection::new();
            self.add_hook_type(&mut types);
            module.section(&types);
        }
        if self.imported_funcs.is_none() && past(SectionId::Import, before) {
            self.imported_funcs = Some(0);
            let mut imports = ImportSection::new();
            self.add_hook(&mut imports);
            module.section(&imports);
        }
        if self.budget.is_none() && past(SectionId::Global, before) {
            let mut globals = GlobalSection::new();
            self.add_budget(&mut globals, self.imported_globals);
            module.section(&globals);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print(wasm: &[u8]) -> String {
        wasmprinter::print_bytes(wasm).unwrap()
    }

    #[test]
    fn checks_go_at_function_entries_and_loop_headers() {
        let wasm = wat::parse_str(
            r#"(module
                (import "wasixv2" "sched_yield" (func (result i32)))
                (func (export "spin") (loop (br 0)))
                (func (drop (call 0)) (call 1))
                (elem declare func 1)
                (start 3)
                (func))"#,
        )
        .unwrap();
        let out = instrument(&wasm);
        wasmparser::validate(&out).unwrap();
        let text = print(&out);
        assert!(
            text.contains(r#"(import "wasixv2-exec" "preempt" (func (;1;) (type"#),
            "{text}"
        );
        // The defined functions moved up by four; `$call` still calls `$spin`.
        assert!(text.contains("call 5"), "{text}");
        assert!(text.contains("(export \"spin\" (func 5))"), "{text}");
        assert!(text.contains("(start 7)"), "{text}");
        assert_eq!(text.matches("call 1").count(), 3, "{text}");
        assert!(
            text.contains("(global (;0;) (mut i32) i32.const 65536)"),
            "{text}"
        );
    }

    #[test]
    fn a_module_without_imports_or_globals_gets_them() {
        let wasm =
            wat::parse_str(r#"(module (memory 1) (func (export "_start") (loop)))"#).unwrap();
        let out = instrument(&wasm);
        wasmparser::validate(&out).unwrap();
        let text = print(&out);
        assert!(
            text.contains(r#"(import "wasixv2-exec" "preempt""#),
            "{text}"
        );
        assert!(text.contains("(export \"_start\" (func 4))"), "{text}");
    }

    #[test]
    fn atomics_on_a_shared_memory_become_calls_with_their_offset() {
        let wasm = wat::parse_str(
            r#"(module
                (import "env" "memory" (memory 1 1 shared))
                (func (export "f") (result i32)
                  (memory.atomic.wait32 offset=8 (i32.const 0) (i32.const 0) (i64.const -1))
                  (memory.atomic.wait64 (i32.const 0) (i64.const 0) (i64.const -1))
                  (memory.atomic.notify offset=4 (i32.const 0) (i32.const 1))
                  i32.add
                  i32.add)
                (start 1)
                (func (drop (memory.atomic.notify (i32.const 0) (i32.const 1)))))"#,
        )
        .unwrap();
        let out = instrument(&wasm);
        wasmparser::validate(&out).unwrap();
        let text = print(&out);
        for name in [WAIT32_NAME, WAIT64_NAME, NOTIFY_NAME] {
            assert!(
                text.contains(&format!(r#"(import "wasixv2-exec" "{name}""#)),
                "{text}"
            );
        }
        assert!(text.contains("i32.const 8\n    call 1"), "{text}");
        assert!(text.contains("i32.const 0\n    call 2"), "{text}");
        assert!(text.contains("i32.const 4\n    call 3"), "{text}");
        // The start function keeps its instruction.
        assert_eq!(text.matches("memory.atomic.").count(), 1, "{text}");
    }

    #[test]
    fn atomics_on_an_unshared_memory_stay() {
        let wasm = wat::parse_str(
            r#"(module
                (memory 1)
                (func (export "f") (result i32)
                  (memory.atomic.notify (i32.const 0) (i32.const 1))))"#,
        )
        .unwrap();
        let out = instrument(&wasm);
        wasmparser::validate(&out).unwrap();
        assert!(print(&out).contains("memory.atomic.notify"));
    }
}
