//! `stack_checkpoint`, `stack_restore`: `setjmp` and `longjmp` of wasix-libc's asyncify builds,
//! on the execution layer's asyncify machinery (`wasixv2_exec::asyncify`), as v2's
//! `stack-save` and `stack-restore` are.
//!
//! `stack_checkpoint` saves the stack under the snapshot's guest address and returns 0 through
//! `ret`; `stack_restore` discards the stack down to the call that saved it and rewinds the
//! saved one, whose `stack_checkpoint` then returns `val`. The snapshot holds the saved
//! stack's id: its slot, generation and level.

use wasixv2_exec::asyncify::{
    StackId,
    import::{self, Resumed},
};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{Errno, proc::StackSnapshot},
    shim::{self, Done, Env},
};

fn snapshot(id: StackId) -> StackSnapshot {
    StackSnapshot {
        user: u64::from(id.slot),
        hash: [id.generation, u64::from(id.level)],
    }
}

fn stack_id(snapshot: &StackSnapshot) -> StackId {
    StackId {
        slot: snapshot.user as u32,
        generation: snapshot.hash[0],
        level: snapshot.hash[1] as u32,
    }
}

/// `stack_checkpoint(snapshot, ret)`. Without asyncify there is no way to save a wasm stack:
/// `NOTSUP`.
pub(crate) async fn stack_checkpoint<S: Syscalls + Control + 'static>(
    env: Env<S>,
    snapshot_ptr: i32,
    ret: i32,
) -> Result<i32, RuntimeError> {
    let (snapshot_ptr, ret) = (snapshot_ptr as u32, ret as u32);
    match import::resumed(&env).await? {
        None => {
            if !import::instrumented(&env).await {
                return Ok(i32::from(Errno::NOTSUP.0));
            }
            // The guest unwinds now and calls again once the stack is saved.
            import::save(&env, snapshot_ptr).await?;
            Ok(0)
        }
        Some(Resumed::Saved(id)) => {
            shim::errno(&env, async |frame| {
                frame.write(snapshot_ptr, &snapshot(id)).await?;
                frame.write(ret, &0u64).await
            })
            .await
        }
        Some(Resumed::Restored(val)) => {
            let val = u64::from(val as u32);
            shim::errno(&env, async |frame| frame.write(ret, &val).await).await
        }
        Some(Resumed::Forked { .. }) => {
            Err(RuntimeError::new("stack_checkpoint resumed from a fork"))
        }
    }
}

/// `stack_restore(snapshot, val)`: never returns.
pub(crate) async fn stack_restore<S: Syscalls + Control + 'static>(
    env: Env<S>,
    snapshot_ptr: i32,
    val: i64,
) -> Result<(), RuntimeError> {
    let read = shim::call(&env, async |frame| {
        frame.read::<StackSnapshot>(snapshot_ptr as u32).await
    })
    .await?;
    let Done::Ok(saved) = read else {
        return Err(RuntimeError::new("stack_restore: unreadable snapshot"));
    };
    Err(import::restore(&env, stack_id(&saved), val as i32).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trip() {
        let id = StackId {
            slot: 0x1234,
            generation: 7,
            level: 2,
        };
        assert_eq!(stack_id(&snapshot(id)), id);
    }
}
