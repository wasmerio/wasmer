pub(crate) mod external;
pub(crate) mod function;
pub(crate) mod global;
pub(crate) mod memory;
pub(crate) mod table;
pub use super::error::Trap;

pub use external::*;
pub use function::*;
pub use global::*;
pub use memory::*;
pub use table::*;

/// The type of instances in the `js` VM.
pub type VMInstance = js_sys::WebAssembly::Instance;

pub struct VMTrampoline;

/// The type of extern tables in the `js` VM.
pub(crate) type VMExternTable = VMTable;
/// The type of extern memories in the `js` VM.
pub(crate) type VMExternMemory = VMMemory;
/// The type of extern globals in the `js` VM.
pub(crate) type VMExternGlobal = VMGlobal;
/// The type of extern functions in the `js` VM.
pub(crate) type VMExternFunction = VMFunction;

/// The type of function callbacks in the `js` VM.
pub type VMFunctionCallback = *const VMFunctionBody;

/// Shared VM memory, in `js`, is the "normal" memory.
pub type VMSharedMemory = VMMemory;
