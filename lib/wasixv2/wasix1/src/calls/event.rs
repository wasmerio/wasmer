//! `fd_event`: an eventfd.

use wasixv2_abi::EventfdFlags;
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::sock::Eventfdflags,
    shim::{self, Env},
};

fn flags(v1: Eventfdflags) -> EventfdFlags {
    let mut flags = 0;
    if v1.0 & Eventfdflags::SEMAPHORE.0 != 0 {
        flags |= EventfdFlags::EFD_SEMAPHORE.0;
    }
    if v1.0 & Eventfdflags::NONBLOCK.0 != 0 {
        flags |= EventfdFlags::EFD_NONBLOCK.0;
    }
    EventfdFlags(flags)
}

/// `fd_event(initial, flags, ro_fd)`. v1's counter starts from a 64-bit value; one beyond
/// `eventfd2`'s 32-bit argument is added with a write, as a guest would.
pub(crate) async fn fd_event<S: Syscalls + Control + 'static>(
    env: Env<S>,
    initial: i64,
    v1_flags: i32,
    ro_fd: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let initial = initial as u64;
        let caller = frame.caller();
        let small = u32::try_from(initial).ok();
        let fd = frame
            .syscalls
            .eventfd2(
                &caller,
                small.unwrap_or(0),
                flags(Eventfdflags(v1_flags as u16)),
            )
            .await?;
        if small.is_none() {
            let value = frame.mem.alloc_bytes(&initial.to_le_bytes())?;
            if let Err(err) = frame.syscalls.write(&caller, fd, value).await {
                // Never blocks: the counter was zero.
                let _ = frame.syscalls.close(&caller, fd).await;
                return Err(err.into());
            }
        }
        frame.write(ro_fd as u32, &(fd.0 as u32)).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_bits() {
        assert_eq!(flags(Eventfdflags(0)), EventfdFlags(0));
        assert_eq!(
            flags(Eventfdflags(
                Eventfdflags::SEMAPHORE.0 | Eventfdflags::NONBLOCK.0
            )),
            EventfdFlags(EventfdFlags::EFD_SEMAPHORE.0 | EventfdFlags::EFD_NONBLOCK.0)
        );
    }
}
