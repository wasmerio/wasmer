//! Borrowed engine authority for the four existing loader imports.
//! Kept separate from UserMemory and Control: a capability belongs to one import future,
//! never to the kernel or guest memory. Default/custom callers remain unsupported.
use crate::{Caller, Errno, LocalFuture, SysResult, UserSliceMut};
use wasixv2_abi::{Cstr, DlFlags, DlHandle, DlHandlePtr, Uptr, UptrPtr};

pub trait DynamicLinker {
    #[allow(clippy::too_many_arguments)]
    fn dl_open<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        file: Cstr,
        flags: DlFlags,
        search_path: Cstr,
        caller_addr: Uptr,
        handle: DlHandlePtr,
        err: UserSliceMut,
    ) -> LocalFuture<'a, SysResult<i32>> {
        let _ = (caller, file, flags, search_path, caller_addr, handle, err);
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }

    fn dl_sym<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        handle: DlHandle,
        symbol: Cstr,
        caller_addr: Uptr,
        addr: UptrPtr,
        err: UserSliceMut,
    ) -> LocalFuture<'a, SysResult<i32>> {
        let _ = (caller, handle, symbol, caller_addr, addr, err);
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }

    fn dl_close<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        handle: DlHandle,
    ) -> LocalFuture<'a, SysResult<i32>> {
        let _ = (caller, handle);
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }

    fn dl_next_init<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        ctors: UptrPtr,
    ) -> LocalFuture<'a, SysResult<i32>> {
        let _ = (caller, ctors);
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }
}
