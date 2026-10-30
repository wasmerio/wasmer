# Browser stdio and TTY transport

`wasixv2-stdio-web` supplies raw, bounded shared-runtime queues through the existing
`wasixv2-host::{ByteSource, ByteSink, Stdio, Console}` interfaces. Its runtime
dependencies are host, wait and futures-util. The kernel dev-dependency is used
only by native integration tests.

Create `(session, stdio, streams)` with `Session::new(capacity)`, pass `stdio` in
`ProcessConfig`, and keep `session` for finalization. `streams.stdin` is the
coordinator producer; `streams.stdout` and `streams.stderr` are separate consumers.
Every direction holds at most `capacity` queued bytes. Writes may be partial;
retry only the unaccepted suffix. A full queue gives `WouldBlock` to `try_write`
or parks an async `ByteSink::write` through its Rust Waker. No task-worker JS
Promise is involved.

`Sink::close` orders persistent EOF after accepted bytes. Last producer drop does
the same. `Source::cancel` or last consumer drop discards queued bytes and wakes
blocked producers with `ConnectionAborted`. `Sink::abort` cancels both sides.
Last consumer drop after clean, fully consumed EOF preserves successful drain;
dropping unread output still fails it. Empty buffers return zero immediately;
a canceled stream otherwise keeps failing.
Dropping a pending async read, write or drain removes its wait registration and
consumes no bytes. `ByteSource::poll_read` supports one logical poll reader,
matching host `BlockingSource`; `Source::read` supports separate async waiters.

These queues are host transports. They do not implement Linux `PIPE_BUF`,
`SIGPIPE`, or descriptor `poll` policy. Use kernel pipes for those semantics
(`fs/pipe.c:anon_pipe_read,anon_pipe_write`).

## Worker and Web Streams merge points

The runtime heap must be shared and built with B2's threaded std recipe. Create
and operate runtime endpoints on task/service/coordinator workers. The UI sends
messages to the independent coordinator; it never enters Rust runtime mutexes,
blocks in `Atomics.wait`, or hosts a kernel.

For a Rust future, use B2's scheduler Waker in its `Context`. For the coordinator's
nonblocking request loop, install `set_wake(Arc<dyn StreamWake>)` **before** testing
`try_read`/`try_write`, publish readiness, and recheck predicates after notification.
`StreamWake::notify` must route to the owning coordinator without touching any
foreign JS handle. A `Waker` implements this trait; B2's `Parker::notify` is also a
straightforward implementation. B2's sequence-before-poll and recheck-before-park
protocol handles notify-before-wait and spurious wakeups. Queue notifications run
with payload and notification-slot mutexes released, like B5's relay queues.

Do not await `Source::read` or `Sink::write` on a plain wasm-bindgen-futures Waker
that schedules a worker-local JS queue from a foreign worker. Use B2's routed
Wakers, or drive the coordinator bridge through `try_*` plus `StreamWake`.

`web-streams.mjs` exports `createStdio(bridge, chunkBytes)`. It provides stdin as a
WritableStream and independent demand-driven stdout/stderr ReadableStreams. Its
bridge interface is documented in the module and maps directly to these endpoints:

| Bridge callback | Coordinator action |
| --- | --- |
| `writeInput(bytes)` | write the bytes to `streams.stdin`, returning a partial count |
| `closeInput()` | `streams.stdin.close()` |
| `abortInput(reason)` | `streams.stdin.abort()` and reject pending input requests |
| `readOutput(name, max)` | read at most `max` bytes from the selected source; map zero to `null` |
| `cancelOutput(name, reason)` | cancel that source and resolve/reject its pending read requests |
| `resize(size)` | existing `Signaller::console_resize(size)` |
| `hangup()` | existing `Signaller::console_hangup()` |

Reads return an owned/transferable Uint8Array, never a runtime scratch view reused
by another operation. Bridge requests and copies must be bounded; do not enqueue
unlimited postMessages or read beyond downstream demand. Output permits one
in-flight chunk per stream and has no read-ahead. Stdin splits input into bounded
chunks and handles partial acceptance without replay. Writable abort's immediate
AbortSignal cancels the coordinator before waiting for an in-flight write, which
otherwise could deadlock on queue capacity. Infrastructure `abort` bypasses JS
reader/writer locks and releases all three endpoints. A canceled output rejects
`outputDrained`; it cannot masquerade as successfully captured output.

B2/B8/B11 must supply the actual shared-runtime coordinator bridge and event-loop
notification implementation. This package does not create workers or export a
second runtime bootstrap. The JS adapter's Promises run exclusively on the
UI/coordinator side, never on a blocking task worker.

## Console and lifecycle

Use `session.console(size)` with `MachineBuilder::console(console,
[true, true, false])` for terminal stdin/stdout and independent piped stderr.
Choose other descriptor masks explicitly when needed. Forward raw key bytes:
0x03 is Ctrl-C and 0x04 is terminal EOF. Do not edit lines, echo, map newlines,
or send SIGINT from JavaScript. The existing kernel pump drives n_tty even when
the guest makes no reads (`drivers/tty/n_tty.c:n_tty_receive_buf_common,n_tty_read`).
Raw termios naturally changes how those same bytes are interpreted.

Resize and hangup are forwarded to the existing Signaller on the coordinator,
which uses the kernel foreground group and signal work flags
(`drivers/tty/tty_io.c:tty_do_resize,do_tty_hangup`,
`kernel/signal.c:complete_signal,signal_wake_up_state`). `createStdio.hangup` also
aborts pending input while preserving output for drain. Host stream EOF is a
terminal hangup; Ctrl-D is a discipline event and does not close its transport.

Consume both outputs concurrently **while the guest runs**, and await downstream
Web Stream writes. After **all tasks sharing the streams** have stopped (including
inherited producers in children), wait for the kernel console's pending writes
and echo with `Kernel::console_wait_until_sent` (the facade's `Machine::run_*`
methods already await this console drain before returning). Then await `session.finish()` while still consuming both outputs. It seals output
and waits until accepted queue bytes reach the coordinator consumers. Await JS
`outputDrained` and the downstream consumers too before returning a final captured
process result. Root process exit alone does not prove that inherited producers
have stopped. Use `session.abort()` for infrastructure failure or teardown that
must discard output and wake every waiter.

B2/B8 must treat TTY/timer service admission failure as a Machine failure. The
existing kernel TTY pump falls back to `spawn_local` on admission failure; that
fallback is forbidden on the blocking browser lane. This package leaves
`kernel/src/tty` unchanged, as required by B7 ownership.

## Validation and remaining browser gate

Native crate tests exercise queue bounds, cancellation, drop/EOF, wake ordering,
cross-thread traffic, independent stderr and dual-output drain. Public-kernel
integration tests cover canonical editing/echo and Ctrl-D, raw control bytes,
Ctrl-C work flags without a guest read, resize/SIGWINCH, background SIGTTIN,
foreground targeting and hangup of a pending read. They follow the existing
`kernel/src/tests/tty.rs` setup without adding a second line discipline.

Run the JS adapter tests with:

```sh
node --test --test-concurrency=3 lib/wasixv2/stdio-web/tests/web-streams.test.mjs
```

These use actual Web Streams with a deterministic coordinator fixture. They do
not claim browser guest execution. B11 must run this bridge with the linked B2/B8
shared-runtime worker harness in the required cross-origin-isolated, no-JSPI
browser lane, including busy-guest Ctrl-C, resize, output saturation/cancel,
exit drain, foreground job control, hangup and worker cleanup. No browser parity
claim follows from the native tests or the wasm library check.
