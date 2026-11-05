//! `random_get`: `getrandom` straight into the guest buffer.

use crate::abi::{GuestSize, Pointer};

use wasixv2_abi::GrndFlags;
use wasixv2_syscalls::{Control, Syscalls, UserSliceMut};
use wasmer::RuntimeError;

use crate::shim::{self, Env};

/// Fills the whole buffer: `getrandom` may return less than asked for.
pub(crate) async fn random_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    buf: Pointer,
    len: GuestSize,
) -> Result<i32, RuntimeError> {
    let mut filled = 0u64;
    shim::errno(&env, async |frame| {
        // Legacy random_get returns before inspecting an empty buffer's address.
        if len.0 == 0 {
            return Ok(());
        }
        frame.check_range(buf, len.0).await?;
        while filled < len.0 {
            let address = buf.offset(filled)?.narrow()?;
            let chunk = (len.0 - filled).min(4096) as u32;
            let rest = UserSliceMut::new(address, chunk);
            filled += u64::from(
                frame
                    .syscalls
                    .getrandom(&frame.caller(), rest, GrndFlags(0))
                    .await?,
            );
        }
        Ok(())
    })
    .await
}
