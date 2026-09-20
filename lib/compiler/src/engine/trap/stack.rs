#[cfg(unix)]
use crate::engine::unwind::EXIT_CALLED;
#[cfg(unix)]
use std::sync::atomic::Ordering;

use super::frame_info::{FRAME_INFO, GlobalFrameInfo};
use backtrace::Backtrace;
use wasmer_types::{FrameInfo, TrapCode};
use wasmer_vm::Trap;

/// Given a `Trap`, this function returns the Wasm trace and the trap code.
pub fn get_trace_and_trapcode(trap: &Trap) -> (Vec<FrameInfo>, Option<TrapCode>) {
    // Host interrupts are synthetic control-plane events. They deliberately
    // carry no backtrace and, crucially, must not wait on FRAME_INFO: module
    // registration may be occurring concurrently while shutdown is draining
    // guests.
    if matches!(
        trap,
        Trap::Lib {
            trap_code: TrapCode::HostInterrupt,
            ..
        } | Trap::Wasm {
            signal_trap: Some(TrapCode::HostInterrupt),
            ..
        }
    ) {
        return (Vec::new(), Some(TrapCode::HostInterrupt));
    }

    #[cfg(unix)]
    // If the exit is called, we can't access the back-trace information any longer (#5877)
    if EXIT_CALLED.load(Ordering::SeqCst) {
        return (Vec::new(), None);
    }

    let info = FRAME_INFO.read().unwrap();
    match &trap {
        // A user error
        Trap::User(_err) => (wasm_trace(&info, None, &Backtrace::new_unresolved()), None),
        // A trap caused by the VM being Out of Memory
        Trap::OOM { backtrace } => (wasm_trace(&info, None, backtrace), None),
        // A trap caused by an error on the generated machine code for a Wasm function
        Trap::Wasm {
            pc,
            signal_trap,
            backtrace,
        } => {
            let trap_code = info
                .lookup_trap_info(*pc)
                .map_or(signal_trap.unwrap_or(TrapCode::StackOverflow), |info| {
                    info.trap_code
                });

            (wasm_trace(&info, Some(*pc), backtrace), Some(trap_code))
        }
        // A trap triggered manually from the Wasmer runtime
        Trap::Lib {
            trap_code,
            backtrace,
        } => (wasm_trace(&info, None, backtrace), Some(*trap_code)),
        // An uncaught exception
        Trap::UncaughtException { backtrace, .. } => (
            wasm_trace(&info, None, backtrace),
            Some(TrapCode::UncaughtException),
        ),
    }
}

/// Captures the current Wasm stack trace. Only useful when
/// there are active Wasm frames on the stack, such as in
/// libcalls or imported functions.
pub fn wasm_trace_from_current_stack() -> Vec<FrameInfo> {
    let info = FRAME_INFO.read().unwrap();
    let backtrace = Backtrace::new_unresolved();
    wasm_trace(&info, None, &backtrace)
}

fn wasm_trace(
    info: &GlobalFrameInfo,
    trap_pc: Option<usize>,
    backtrace: &Backtrace,
) -> Vec<FrameInfo> {
    // Let's construct the trace
    backtrace
        .frames()
        .iter()
        .filter_map(|frame| {
            let pc = frame.ip() as usize;
            if pc == 0 {
                None
            } else {
                // Note that we need to be careful about the pc we pass in here to
                // lookup frame information. This program counter is used to
                // translate back to an original source location in the origin wasm
                // module. If this pc is the exact pc that the trap happened at,
                // then we look up that pc precisely. Otherwise backtrace
                // information typically points at the pc *after* the call
                // instruction (because otherwise it's likely a call instruction on
                // the stack). In that case we want to lookup information for the
                // previous instruction (the call instruction) so we subtract one as
                // the lookup.
                let pc_to_lookup = if Some(pc) == trap_pc { pc } else { pc - 1 };
                Some(pc_to_lookup)
            }
        })
        .filter_map(|pc| info.lookup_frame_info(pc))
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_interrupt_does_not_read_the_frame_registry() {
        // Holding the write side here makes the invariant deterministic: a
        // HostInterrupt must be classified before touching FRAME_INFO or this
        // call would self-deadlock. Interrupts are synthetic control-plane
        // events and intentionally have no guest trace.
        let _frame_registry = FRAME_INFO.write().unwrap();
        let (trace, code) = get_trace_and_trapcode(&Trap::host_interrupt());
        assert!(trace.is_empty());
        assert_eq!(code, Some(TrapCode::HostInterrupt));

        let signal_interrupt = Trap::wasm(
            0,
            Backtrace::from(Vec::new()),
            Some(TrapCode::HostInterrupt),
        );
        let (trace, code) = get_trace_and_trapcode(&signal_interrupt);
        assert!(trace.is_empty());
        assert_eq!(code, Some(TrapCode::HostInterrupt));
    }
}
