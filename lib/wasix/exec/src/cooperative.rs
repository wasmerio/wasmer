//! Host-only loader work while the exact pending guest/syscall future stays pinned.
//!
//! Ptrace's kernel/signal.c:ptrace_stop keeps its syscall future suspended until the
//! tracer resumes it. Cooperation must neither cancel/restart it nor run guest code.

use crate::{AbiInstance, Entry};
use std::{
    future::{Future, poll_fn},
    pin::{Pin, pin},
    task::Poll,
};
use wasixv2_syscalls::{Control, Syscalls};
use wasmer::RuntimeError;

enum Next<T> {
    Guest(T),
    Work,
    Detached,
}

async fn next<G, W>(
    mut guest: Pin<&mut G>,
    mut work: Pin<&mut W>,
    busy: impl Fn() -> bool,
    safe: impl Fn() -> bool,
) -> Next<G::Output>
where
    G: Future,
    W: Future<Output = bool>,
{
    let mut notified = None;
    poll_fn(|cx| {
        // Register the work wake even while the exact syscall future is unsafe to
        // suspend (it may hold mm.op). Remember Ready without repolling a completed
        // work future, and only service once that guest future reaches a safe stop.
        let mut work_ready = |cx: &mut std::task::Context<'_>| {
            if busy() {
                return None;
            }
            if notified.is_none()
                && let Poll::Ready(peer) = work.as_mut().poll(cx)
            {
                notified = Some(peer);
            }
            match notified {
                Some(false) => Some(Next::Detached),
                Some(true) if safe() => Some(Next::Work),
                _ => None,
            }
        };
        if let Some(next) = work_ready(cx) {
            return Poll::Ready(next);
        }
        if let Poll::Ready(result) = guest.as_mut().poll(cx) {
            return Poll::Ready(Next::Guest(result));
        }
        // Polling guest may have entered a ptrace entry/exit stop. Recheck now:
        // that transition need not wake the already-ready work future again.
        work_ready(cx).map_or(Poll::Pending, Poll::Ready)
    })
    .await
}

pub(crate) async fn drive<S, G>(
    entry: &Entry<'_, S>,
    abi: &dyn AbiInstance<S>,
    guest: G,
) -> Result<(), RuntimeError>
where
    S: Syscalls + Control + Send + Sync + 'static,
    G: Future<Output = Result<(), RuntimeError>>,
{
    if !abi.cooperative_loader() {
        return guest.await;
    }
    // One pin for the entire call. Each work round borrows, never replaces this future.
    let mut guest = pin!(guest);
    loop {
        let (syscalls, task) = entry
            .with_store(|env, _| (env.syscalls().clone(), env.task()))
            .await;
        let Some(gate) = syscalls.loader_gate(task) else {
            return guest.await;
        };
        let mut work = pin!(gate.wait_peer_work(task));
        match next(
            guest.as_mut(),
            work.as_mut(),
            || abi.loader_work_busy(),
            || syscalls.loader_cooperative_safe(task),
        )
        .await
        {
            Next::Guest(result) => return result,
            Next::Work => abi.loader_work_outside(entry).await?,
            // exit/exec detached this old attachment. Its original future owns how
            // execution ends or switches; do not watch a permanently ready old gate.
            Next::Detached => return guest.await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        rc::Rc,
        task::{Context, Waker},
    };

    struct Held(Rc<Cell<usize>>);
    impl Drop for Held {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn pending_non_send_guest_future_is_retained_once_across_prioritized_work() {
        let starts = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        let ready = Rc::new(Cell::new(false));
        let mut guest = Box::pin({
            let (starts, drops, ready) = (starts.clone(), drops.clone(), ready.clone());
            async move {
                let _held = Held(drops);
                starts.set(starts.get() + 1);
                poll_fn(|_| {
                    if ready.get() {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                7
            }
        });
        let mut cx = Context::from_waker(Waker::noop());
        let mut idle = pin!(std::future::pending());
        let mut waiting = Box::pin(next(guest.as_mut(), idle.as_mut(), || false, || true));
        assert!(waiting.as_mut().poll(&mut cx).is_pending());
        drop(waiting);
        assert_eq!((starts.get(), drops.get()), (1, 0));
        ready.set(true);
        let mut work = pin!(std::future::ready(true));
        let mut notified = Box::pin(next(guest.as_mut(), work.as_mut(), || false, || true));
        assert!(matches!(
            notified.as_mut().poll(&mut cx),
            Poll::Ready(Next::Work)
        ));
        drop(notified);
        assert_eq!(
            (starts.get(), drops.get()),
            (1, 0),
            "work runs before ready guest instructions"
        );
        let mut resumed = Box::pin(next(guest.as_mut(), idle.as_mut(), || false, || true));
        assert!(matches!(
            resumed.as_mut().poll(&mut cx),
            Poll::Ready(Next::Guest(7))
        ));
        assert_eq!((starts.get(), drops.get()), (1, 1));
    }

    #[test]
    fn unsafe_pending_syscall_keeps_polling_until_safe_stop_without_repolling_work() {
        let safe = Cell::new(false);
        let polls = Cell::new(0);
        let work_polls = Cell::new(0);
        let mut guest = pin!(poll_fn(|_| {
            polls.set(polls.get() + 1);
            if polls.get() == 2 {
                safe.set(true);
            }
            Poll::<()>::Pending
        }));
        let mut work = pin!(poll_fn(|_| {
            work_polls.set(work_polls.get() + 1);
            assert_eq!(work_polls.get(), 1, "completed work future was repolled");
            Poll::Ready(true)
        }));
        let mut selected = pin!(next(guest.as_mut(), work.as_mut(), || false, || safe.get()));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(selected.as_mut().poll(&mut cx).is_pending());
        assert_eq!(
            polls.get(),
            1,
            "unsafe syscall must keep its lock-owning future"
        );
        assert!(matches!(
            selected.as_mut().poll(&mut cx),
            Poll::Ready(Next::Work)
        ));
        assert_eq!(polls.get(), 2, "stop reached while guest was polled");
    }

    #[test]
    fn existing_loader_service_is_polled_instead_of_reentered() {
        let work_polls = Cell::new(0);
        let mut work = pin!(poll_fn(|_| {
            work_polls.set(work_polls.get() + 1);
            Poll::Ready(true)
        }));
        let mut guest = pin!(std::future::ready(9));
        let mut selected = pin!(next(guest.as_mut(), work.as_mut(), || true, || true));
        assert!(matches!(
            selected
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Ready(Next::Guest(9))
        ));
        assert_eq!(work_polls.get(), 0);
    }
}
