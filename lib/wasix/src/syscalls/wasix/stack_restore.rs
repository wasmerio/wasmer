use super::*;
use crate::syscalls::*;

/// ### `stack_restore()`
/// Restores the current stack to a previous stack described by its
/// stack hash.
///
/// ## Parameters
///
/// * `snapshot_ptr` - Contains a previously made snapshot
#[instrument(level = "trace", skip_all, ret)]
pub fn stack_restore<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    snapshot_ptr: WasmPtr<StackSnapshot, M>,
    mut val: Longsize,
) -> Result<(), WasiError> {
    // Read the snapshot from the stack
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let snapshot = match snapshot_ptr.read(&memory) {
        Ok(a) => {
            trace!("with_ret={}, hash={}, user={}", val, a.hash(), a.user);
            a
        }
        Err(err) => {
            warn!("failed to read stack snapshot - {}", err);
            return Err(WasiError::Exit(mem_error_to_wasi(err).into()));
        }
    };

    // Perform the unwind action
    unwind::<M, _>(ctx, move |mut ctx, _, _| {
        // Let the stack (or fail trying!)
        let env = ctx.data();
        if let Some((mut memory_stack, rewind_stack, store_data)) =
            env.thread.get_snapshot(snapshot.hash())
        {
            let env = ctx.data();
            let memory = unsafe { env.memory_view(&ctx) };

            // Rewind the stack - after this point we must immediately return
            // so that the execution can end here and continue elsewhere.
            let pid = ctx.data().pid();
            let tid = ctx.data().tid();

            let rewind_result = bincode::serialize(&val).unwrap().into();
            let ret = rewind_ext::<M>(
                &mut ctx,
                None, // we do not restore the thread memory as `longjmp`` is not meant to do this
                rewind_stack,
                store_data,
                RewindResultType::RewindWithResult(rewind_result),
            );
            match ret {
                Errno::Success => OnCalledAction::InvokeAgain,
                err => {
                    warn!("failed to rewind the stack - errno={}", err);
                    OnCalledAction::Trap(Box::new(WasiError::Exit(err.into())))
                }
            }
        } else {
            warn!(
                "snapshot stack restore failed - the snapshot can not be found and hence restored (hash={})",
                snapshot.hash()
            );
            OnCalledAction::Trap(Box::new(WasiError::Exit(Errno::Unknown.into())))
        }
    });

    // Return so the stack can be unwound (which will then
    // be rewound again but with a different location)
    Ok(())
}
