mod code;
pub mod intrinsics;
mod state;
pub mod trampoline;

pub use self::code::FuncTranslator;
pub use self::trampoline::FuncTrampoline;

// Use a consistent alignment for all defined output functions to keep their layout/addressing predictable across builds.
const FUNCTION_ALIGNMENT: u32 = 16;
