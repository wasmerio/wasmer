//! Engine-free function authority borrowed by one import invocation.
use crate::{Caller, Errno, LocalFuture, RawGuestBuffer, RawGuestPointer, SysResult, UserSlice};
use wasixv2_abi::{CallDynamicFlags, U32Ptr, Uptr};

#[allow(unused_variables)]
pub trait FunctionOperations {
    fn call_dynamic<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        function: u32,
        values: RawGuestBuffer,
        results: RawGuestBuffer,
        flags: CallDynamicFlags,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }

    fn reflect_signature<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        function: u32,
        argument_types: RawGuestBuffer,
        result_types: RawGuestBuffer,
        result: RawGuestPointer,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }

    fn closure_allocate<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        closure: U32Ptr,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }

    fn closure_prepare<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        backing: u32,
        closure: u32,
        argument_types: UserSlice,
        result_types: UserSlice,
        env_: Uptr,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }

    fn closure_free<'a>(
        &'a self,
        caller: &'a Caller<'_>,
        closure: u32,
    ) -> LocalFuture<'a, SysResult<i32>> {
        Box::pin(async { Err(Errno::ENOSYS.into()) })
    }
}
