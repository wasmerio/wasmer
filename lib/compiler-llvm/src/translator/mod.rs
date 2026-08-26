mod code;
pub mod intrinsics;
mod state;
pub mod trampoline;

pub use self::code::FuncTranslator;
pub use self::trampoline::FuncTrampoline;

// Use the same alignment for all output functions to prevent the linker from reordering their symbols.
const FUNCTION_ALIGNMENT: u32 = 16;
