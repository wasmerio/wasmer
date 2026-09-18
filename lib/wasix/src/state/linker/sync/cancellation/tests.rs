use std::{
    future::{Future, poll_fn},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use tokio::sync::{Barrier, oneshot};
use wasmer::Store;

use super::*;

const DEADLINE: Duration = Duration::from_secs(2);

fn env() -> WasiEnv {
    let store = Store::default();
    WasiEnv::builder("linker-cancellation")
        .engine(store.engine().clone())
        .build()
        .unwrap()
}

fn assert_aborted<T>(result: Result<T, LinkError>, expected: ExitCode) {
    match result {
        Err(LinkError::SynchronizationAborted(actual)) => assert_eq!(actual, expected),
        _ => panic!("linker operation did not abort with the expected exit code"),
    }
}

struct Worker {
    done: mpsc::Receiver<Result<(), LinkError>>,
    release: oneshot::Sender<()>,
    handle: thread::JoinHandle<()>,
}

impl Worker {
    fn finish(self) -> Result<(), LinkError> {
        let result = self.done.recv_timeout(DEADLINE);
        if result.is_err() {
            // The fallback makes a deliberately broken cancellation wait fail
            // without leaving a native worker parked in the test process.
            let _ = self.release.send(());
            let _ = self.done.recv_timeout(DEADLINE);
            self.handle.join().unwrap();
            panic!("linker cancellation did not release its native waiter");
        }
        self.handle.join().unwrap();
        result.unwrap()
    }
}

fn spawn_wait(
    cancellation: LinkerCancellation,
    env: WasiEnv,
    work: impl Future<Output = ()> + Send + 'static,
) -> (Worker, mpsc::Receiver<()>) {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let handle = thread::spawn(move || {
        let mut work = Box::pin(async move {
            tokio::select! {
                () = work => {},
                _ = release_rx => {},
            }
        });
        let mut entered_tx = Some(entered_tx);
        let result = cancellation.wait(
            &env,
            poll_fn(move |cx| {
                if let Some(sender) = entered_tx.take() {
                    let _ = sender.send(());
                }
                work.as_mut().poll(cx)
            }),
        );
        let _ = done_tx.send(result);
    });
    (
        Worker {
            done: done_rx,
            release: release_tx,
            handle,
        },
        entered_rx,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leader_termination_aborts_first_epoch_and_forbids_reuse() {
    let leader = env();
    let process = leader.process.clone();
    let cancellation = LinkerCancellation::new();
    let barrier = Arc::new(Barrier::new(2));
    let (worker, entered) = spawn_wait(cancellation.clone(), leader, async move {
        barrier.wait().await;
    });
    entered.recv_timeout(DEADLINE).unwrap();
    process.force_terminate(137.into()).unwrap();
    assert_aborted(worker.finish(), 137.into());

    // Even a different still-running participant cannot use partially changed
    // tables, and a ready future must never be polled after the sticky abort.
    let peer = env();
    let mut polled = false;
    assert_aborted(
        cancellation.wait(&peer, async { polled = true }),
        137.into(),
    );
    assert!(!polled);
    assert_aborted(cancellation.check(&peer), 137.into());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn follower_termination_aborts_second_epoch() {
    let follower = env();
    let process = follower.process.clone();
    let cancellation = LinkerCancellation::new();
    let barrier = Arc::new(Barrier::new(2));
    let follower_barrier = barrier.clone();
    let (second_tx, second_rx) = mpsc::channel();
    let (worker, entered) = spawn_wait(cancellation.clone(), follower, async move {
        follower_barrier.wait().await;
        let _ = second_tx.send(());
        follower_barrier.wait().await;
    });
    entered.recv_timeout(DEADLINE).unwrap();
    block_on(barrier.wait());
    second_rx.recv_timeout(DEADLINE).unwrap();
    process.force_terminate(138.into()).unwrap();
    assert_aborted(worker.finish(), 138.into());
    assert_aborted(cancellation.check(&env()), 138.into());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abandoned_leader_wakes_an_unterminated_peer() {
    let cancellation = LinkerCancellation::new();
    let leader = cancellation.guard();
    let (worker, entered) = spawn_wait(cancellation.clone(), env(), std::future::pending());
    entered.recv_timeout(DEADLINE).unwrap();
    drop(leader);
    assert_aborted(worker.finish(), Errno::Noexec.into());
    // Later failures cannot change the canonical reason.
    assert_aborted::<()>(Err(cancellation.abort(139.into())), Errno::Noexec.into());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_is_published_before_dropping_a_rendezvous_waiter() {
    struct CheckAbortOnDrop {
        cancellation: LinkerCancellation,
        observed: Arc<std::sync::atomic::AtomicBool>,
    }
    impl Drop for CheckAbortOnDrop {
        fn drop(&mut self) {
            self.observed.store(
                self.cancellation.aborted.borrow().is_some(),
                std::sync::atomic::Ordering::SeqCst,
            );
        }
    }
    let cancellation = LinkerCancellation::new();
    let participant = env();
    let process = participant.process.clone();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let on_drop = CheckAbortOnDrop {
        cancellation: cancellation.clone(),
        observed: observed.clone(),
    };
    let (worker, entered) = spawn_wait(cancellation, participant, async move {
        let _on_drop = on_drop;
        std::future::pending::<()>().await;
    });
    entered.recv_timeout(DEADLINE).unwrap();
    process.force_terminate(137.into()).unwrap();
    assert_aborted(worker.finish(), 137.into());
    assert!(
        observed.load(std::sync::atomic::Ordering::SeqCst),
        "a non-cancel-safe waiter was dropped before peers could observe abort"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abort_between_first_epoch_and_publication_releases_mailbox_receiver() {
    let cancellation = LinkerCancellation::new();
    let leader = cancellation.guard();
    let mut mailbox = bus::Bus::<u32>::new(1);
    let mut receiver = mailbox.add_rx();
    let follower = env();
    let peer_cancellation = cancellation.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        entered_tx.send(()).unwrap();
        let _ = done_tx.send(peer_cancellation.recv(&follower, &mut receiver));
    });
    entered_rx.recv_timeout(DEADLINE).unwrap();
    drop(leader);
    let result = done_rx.recv_timeout(DEADLINE);
    if result.is_err() {
        // Old blocking recv can be released without leaving a native thread.
        mailbox.broadcast(1);
        let _ = done_rx.recv_timeout(DEADLINE);
    }
    worker.join().unwrap();
    assert_aborted(
        result.expect("mailbox receive ignored linker abort"),
        Errno::Noexec.into(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successful_two_epoch_rounds_remain_reusable() {
    let cancellation = LinkerCancellation::new();
    let leader = env();
    let follower = env();
    let barrier = Arc::new(Barrier::new(2));
    let follower_barrier = barrier.clone();
    let follower_cancellation = cancellation.clone();
    let worker = thread::spawn(move || {
        for _ in 0..16 {
            let replay = follower_cancellation.guard();
            follower_cancellation
                .wait(&follower, follower_barrier.wait())
                .unwrap();
            follower_cancellation
                .wait(&follower, follower_barrier.wait())
                .unwrap();
            replay.complete();
        }
    });
    for _ in 0..16 {
        let replay = cancellation.guard();
        cancellation.wait(&leader, barrier.wait()).unwrap();
        cancellation.wait(&leader, barrier.wait()).unwrap();
        replay.complete();
    }
    worker.join().unwrap();
    cancellation.check(&leader).unwrap();
}
