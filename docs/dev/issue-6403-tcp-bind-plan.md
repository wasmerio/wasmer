# Issue #6403 Plan: Allocate TCP Ephemeral Ports at `bind()`

Issue: [wasmerio/wasmer#6403](https://github.com/wasmerio/wasmer/issues/6403)

## Problem Summary

For TCP sockets in Wasix, `bind(("host", 0))` currently stores the requested address but does not perform a real backend bind. As a result:

- `getsockname()` after `bind()` still reports port `0`
- the kernel-assigned ephemeral port only appears after `listen()`
- code that relies on POSIX behavior breaks

Native systems allocate the ephemeral port at `bind()` time, not at `listen()` time.

## Root Cause

The relevant behavior is in `lib/wasix/src/net/socket.rs`.

- `InodeSocket::bind()` stores the requested TCP address for `PreSocket` / `RemoteSocket` stream sockets and returns without calling into the networking backend.
- `InodeSocket::addr_local()` for those pre-listen socket states simply returns the stored address, which remains `host:0`.
- `InodeSocket::listen()` is the first place that actually calls `net.listen_tcp(...)`, so the real OS bind and ephemeral port allocation happen too late.

This is not just a `getsockname()` reporting bug. The underlying port is not actually reserved until `listen()`.

## Secondary Constraint

The current `virtual-net` abstraction exposes:

- `listen_tcp(...)`
- `bind_udp(...)`
- `connect_tcp(...)`

but it does not expose a TCP bind primitive that can:

- perform a real bind without listening yet
- report the effective local address after binding
- later transition into `listen()` or `connect()`

That means the fix needs to extend the backend abstraction rather than only patching Wasix-local state.

## Proposed Fix

### 1. Add a regression test first

Add a new socket test under `lib/wasix/tests/wasm_tests/socket_tests/` that:

1. creates an IPv4 TCP socket
2. binds to `127.0.0.1:0`
3. checks that `getsockname().port != 0` immediately after `bind()`
4. calls `listen()`
5. checks that the port stays the same after `listen()`

This locks in the POSIX behavior expected by the issue report.

## 2. Introduce a real TCP-bound socket state in `virtual-net`

Extend `lib/virtual-net` with a TCP bind API and a corresponding bound-socket type that can:

- return `addr_local()`
- transition into a TCP listener
- transition into a TCP stream connection

At a minimum, the new backend capability needs to preserve the actual local port selected during `bind()`.

## 3. Implement the new backend path

### Host backend

Update `lib/virtual-net/src/host.rs` to create a TCP socket explicitly, apply socket options, call `bind()`, and read back the effective local address before any later `listen()` or `connect()` step.

This likely requires `socket2`, similar to the existing UDP bind implementation.

### Loopback backend

Update `lib/virtual-net/src/loopback.rs` so a TCP bind to port `0` allocates an ephemeral port during bind, rather than preserving `0` until listen.

### Remote client/server backend

Update `lib/virtual-net/src/meta.rs`, `client.rs`, and `server.rs` to carry the new TCP bind operation across the remote networking protocol.

Without this, Wasix behavior will diverge depending on which backend is active.

## 4. Update the Wasix socket state machine

In `lib/wasix/src/net/socket.rs`:

- make TCP `bind()` return a real upgraded socket object instead of `Ok(None)`
- add a socket state representing “TCP socket bound locally but not yet listening/connected”
- make `addr_local()` read the effective address from that bound socket state
- make `listen()` consume the bound socket instead of rebinding from scratch
- make `connect()` also honor the previously bound local address

This keeps bind/listen/connect semantics aligned and avoids reporting a port that is not actually reserved.

## 5. Fix journaling semantics

`lib/wasix/src/syscalls/wasix/sock_bind.rs` currently journals the requested address from guest memory, which is wrong for `bind(port=0)`.

After the functional fix:

- query the effective local address after `sock_bind` succeeds
- journal that effective address instead of the requested `host:0`

Otherwise journal replay can observe a different port from the one the program originally saw.

## Implementation Order

1. Add the Wasix regression test for `bind(..., 0)` + `getsockname()`.
2. Add the new TCP bind abstraction in `virtual-net`.
3. Implement the host backend first.
4. Update Wasix socket state transitions to use the real bound socket.
5. Update journaling to store the effective address.
6. Extend loopback and remote client/server backends.
7. Run targeted socket tests and any relevant `virtual-net` tests.

## Non-Goals

- Faking `getsockname()` by inventing a port in Wasix state without actually reserving it
- Fixing only the listen path while leaving bind-then-connect semantics inconsistent
- Fixing only the host backend and leaving other `virtual-net` backends with different behavior

## Expected Outcome

After the fix:

- `bind(("127.0.0.1", 0))` allocates a real ephemeral port immediately
- `getsockname()` reports the assigned port right after `bind()`
- `listen()` keeps the same local port
- journal replay preserves the same observed bound address
