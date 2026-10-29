//! Kernel ids (`include/linux/uidgid.h`): the uids and gids the kernel stores and compares,
//! which are what the initial user namespace sees. A guest names ids in its own user namespace;
//! they become kernel ids where they enter the kernel ([`crate::ns::UserNs::make_kuid`]) and
//! go back where they leave it ([`crate::ns::UserNs::munged_uid`]).

/// `kuid_t`: a uid as the initial user namespace sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Kuid(pub(crate) u32);

/// `kgid_t`: a gid as the initial user namespace sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Kgid(pub(crate) u32);

impl Kuid {
    /// `GLOBAL_ROOT_UID`: root of the initial user namespace.
    pub(crate) const GLOBAL_ROOT: Kuid = Kuid(0);
}

/// `overflowuid` and `overflowgid` (`DEFAULT_OVERFLOWUID`): what a uid or gid shows as where
/// the reader's user namespace has no mapping for it.
pub(crate) const OVERFLOW_ID: u32 = 65534;

/// `(uid_t)-1`: "unchanged" for the set*id calls, and never an id.
pub(crate) const NO_ID: u32 = u32::MAX;
