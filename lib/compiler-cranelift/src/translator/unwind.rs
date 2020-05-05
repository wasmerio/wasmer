//! A `Compilation` contains the compiled function bodies for a WebAssembly
//! module.

use cranelift_codegen::{isa, Context};
use wasmer_compiler::{CompiledFunctionUnwindInfo, FDERelocEntry};

/// Constructs unwind info object from Cranelift IR
pub fn compiled_function_unwind_info(
    isa: &dyn isa::TargetIsa,
    context: &Context,
) -> CompiledFunctionUnwindInfo {
    use cranelift_codegen::binemit::{FrameUnwindKind, FrameUnwindOffset, FrameUnwindSink, Reloc};
    use cranelift_codegen::isa::CallConv;

    struct Sink(Vec<u8>, usize, Vec<FDERelocEntry>);
    impl FrameUnwindSink for Sink {
        fn len(&self) -> FrameUnwindOffset {
            self.0.len()
        }
        fn bytes(&mut self, b: &[u8]) {
            self.0.extend_from_slice(b);
        }
        fn reserve(&mut self, len: usize) {
            self.0.reserve(len)
        }
        fn reloc(&mut self, r: Reloc, off: FrameUnwindOffset) {
            self.2.push(FDERelocEntry(
                0,
                off,
                match r {
                    Reloc::Abs4 => 4,
                    Reloc::Abs8 => 8,
                    _ => {
                        panic!("unexpected reloc type");
                    }
                },
            ))
        }
        fn set_entry_offset(&mut self, off: FrameUnwindOffset) {
            self.1 = off;
        }
    }

    let kind = match context.func.signature.call_conv {
        CallConv::SystemV | CallConv::Fast | CallConv::Cold => FrameUnwindKind::Libunwind,
        CallConv::WindowsFastcall => FrameUnwindKind::Fastcall,
        _ => {
            return CompiledFunctionUnwindInfo::None;
        }
    };

    let mut sink = Sink(Vec::new(), 0, Vec::new());
    context.emit_unwind_info(isa, kind, &mut sink);

    let Sink(data, offset, relocs) = sink;
    if data.is_empty() {
        return CompiledFunctionUnwindInfo::None;
    }

    match kind {
        FrameUnwindKind::Fastcall => CompiledFunctionUnwindInfo::Windows(data),
        FrameUnwindKind::Libunwind => CompiledFunctionUnwindInfo::FrameLayout(data, offset, relocs),
    }
}
