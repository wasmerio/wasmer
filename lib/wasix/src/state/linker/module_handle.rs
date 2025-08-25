use derive_more::Debug;

// IDs for special module handles
// Not public to ensure they are only used through the safe API.
// TODO: These should probably move to the C bindings at some point

/// Constant representing the RTLD_DEFAULT flag (0) - for searching in all loaded objects
const RAW_MODULE_HANDLE_RTLD_DEFAULT: u32 = 0;
/// Constant representing the main module ID (1)
const RAW_MODULE_HANDLE_MAIN: u32 = 1;
/// Constant representing an invalid module handle (u32::MAX)
const RAW_MODULE_HANDLE_INVALID: u32 = u32::MAX;

/// A handle of a dynamic loaded shared object returned by dlopen
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModuleHandleWithFlags {
    /// An invalid module handle
    Invalid,
    /// This special handle means "search all objects" in dlsym
    RtldDefault,
    /// A normal module handle
    Normal(ModuleHandle),
    // /// This special handle means "search in all objects after the current one" in `dlsym`
    // pub const RTLD_NEXT: ModuleHandle = ModuleHandle(u32::MAX);
}

/// A module handle
///
/// Guaranteed not to be any special handle like RTLD_DEFAULT, RTLD_NEXT or INVALID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleHandle {
    /// The raw module ID as a u32
    ///
    /// Not public to ensure this is only constructed through the safe API.
    id: u32,
}

impl ModuleHandle {
    /// Module handle 1 is always the main module.
    ///
    /// Side modules get handles starting from the next one after the main module.
    ///
    /// This is the lowest valid module handle.
    pub const MAIN: Self = ModuleHandle {
        id: RAW_MODULE_HANDLE_MAIN,
    };
    /// Get the next module handle after this one
    ///
    /// Returns `None` if there is no valid id after this one.
    pub(super) fn next(&self) -> Option<ModuleHandle> {
        let next_id = self.id.checked_add(1)?;
        let next_handle = ModuleHandleWithFlags::from(next_id);
        match next_handle {
            ModuleHandleWithFlags::Normal(valid_id) => Some(valid_id),
            _ => None,
        }
    }
}

impl std::fmt::Display for ModuleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModuleId({})", self.id)
    }
}

impl std::fmt::Display for ModuleHandleWithFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleHandleWithFlags::Invalid => write!(f, "ModuleHandle::Invalid"),
            ModuleHandleWithFlags::RtldDefault => write!(f, "ModuleHandle::RtldDefault"),
            ModuleHandleWithFlags::Normal(id) => write!(f, "ModuleHandle::{id}"),
        }
    }
}

// All conversions between ModuleHandle, ModuleHandleWithFlags and u32

impl TryFrom<ModuleHandleWithFlags> for ModuleHandle {
    type Error = ();

    /// Try to convert from ModuleHandleWithFlags to ModuleHandle
    ///
    /// Will fail if the ModuleHandleWithFlags represents a special handle.
    fn try_from(handle: ModuleHandleWithFlags) -> Result<Self, Self::Error> {
        match handle {
            ModuleHandleWithFlags::Normal(id) => Ok(id),
            _ => Err(()),
        }
    }
}
impl TryFrom<u32> for ModuleHandle {
    type Error = ();

    /// Try to convert from u32 to ModuleHandle
    ///
    /// Will fail if the u32 represents a special handle.
    fn try_from(handle: u32) -> Result<Self, Self::Error> {
        ModuleHandleWithFlags::from(handle).try_into()
    }
}
impl From<ModuleHandle> for u32 {
    /// Convert a ModuleHandle to its raw u32 ID
    fn from(id: ModuleHandle) -> Self {
        id.id
    }
}

impl From<ModuleHandle> for ModuleHandleWithFlags {
    /// Convert a ModuleHandle to a ModuleHandleWithFlags::ModuleId variant
    fn from(id: ModuleHandle) -> Self {
        ModuleHandleWithFlags::Normal(id)
    }
}
impl From<ModuleHandleWithFlags> for u32 {
    /// Convert ModuleHandleWithFlags to its raw u32 representation
    fn from(handle: ModuleHandleWithFlags) -> Self {
        match handle {
            ModuleHandleWithFlags::Invalid => RAW_MODULE_HANDLE_INVALID,
            ModuleHandleWithFlags::RtldDefault => RAW_MODULE_HANDLE_RTLD_DEFAULT,
            ModuleHandleWithFlags::Normal(id) => id.into(),
        }
    }
}
impl From<u32> for ModuleHandleWithFlags {
    /// Convert a raw u32 handle value to the appropriate ModuleHandleWithFlags variant
    fn from(handle: u32) -> Self {
        match handle {
            RAW_MODULE_HANDLE_INVALID => ModuleHandleWithFlags::Invalid,
            RAW_MODULE_HANDLE_RTLD_DEFAULT => ModuleHandleWithFlags::RtldDefault,
            id => ModuleHandleWithFlags::Normal(ModuleHandle { id }),
        }
    }
}
