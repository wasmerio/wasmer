//! `random_get`: `getrandom` straight into the guest buffer.

use wasixv2_abi::GrndFlags;
use wasixv2_syscalls::{Control, Syscalls, UserSliceMut};
use wasmer::RuntimeError;

use crate::shim::{self, Env};

/// Fills the whole buffer: `getrandom` may return less than asked for.
pub(crate) async fn random_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    buf: i32,
    len: i32,
) -> Result<i32, RuntimeError> {
    let (buf, len) = (buf as u32, len as u32);
    let mut filled = 0;
    shim::errno(&env, async |frame| {
        while filled < len {
            let rest = UserSliceMut::new(buf.wrapping_add(filled), len - filled);
            filled += frame
                .syscalls
                .getrandom(&frame.caller(), rest, GrndFlags(0))
                .await?;
        }
        Ok(())
    })
    .await
}
