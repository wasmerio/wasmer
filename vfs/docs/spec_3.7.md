IMPORTANT NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan!!!


## Step 3.7 Spec: IO Rate Limiting (IOPS / Throughput)

### Goal

Add a **low-overhead, pluggable rate-limiting subsystem** that can throttle filesystem operations by:

* **IOPS** (operations per second)
* **Bandwidth** (bytes per second, read + write separately or combined)

…and can be applied at three scopes:

1. **Global VFS context** (all filesystem ops for a process/context)
2. **Per-mount** (all ops routed through a mountpoint)
3. **Per-Fs instance** (all ops handled by a specific filesystem instance)

It must work on both sync + async paths, support **bursts**, provide **fairness controls**, and integrate into VFS dispatch with **minimal overhead**.

This step is **standalone** (`vfs/ratelimit/`), but it must integrate cleanly with:

* `vfs/core` mount table + path walker (Phase 3.1/3.3)
* `vfs/core` handle/OFD semantics (Phase 2.4)
* sync/async bridging (Phase 4, `vfs/rt`)
* Wasix nonblocking flags + errno mapping (`vfs/unix`) (Phase 5)

---

## Deliverables

### New crate

* `vfs/ratelimit/` (package name: `vfs-ratelimit`)

  * `src/lib.rs`
  * `src/config.rs`
  * `src/cost.rs`
  * `src/limiter.rs` (traits + common types)
  * `src/token_bucket.rs` (primary implementation)
  * `src/fair.rs` (optional fairness layer: per-key queues)
  * `src/time.rs` (time abstraction)
  * `src/rt.rs` (async sleep + cancellation hooks, *no tokio dependency*)
  * `tests/*` (unit + behavior tests)

### Integration hooks (small, targeted changes)

* `vfs/core`:

  * Add optional `RateLimitPolicy` references at **context**, **mount**, and **fs** levels.
  * Insert “acquire tokens” checks in **core operation dispatch** paths (not in backends).
* `vfs/unix`:

  * Ensure throttling maps to `EAGAIN` / `EWOULDBLOCK` when requested.

---

## Non-Goals (explicitly out of scope)

* Kernel-level scheduling / true OS I/O prioritization.
* Perfect fairness under all contention patterns.
* Backends implementing their own throttling (VFS is the canonical enforcement layer).
* Persisting limiter state across process restarts.

---

## Key design constraints from the overall plan

### 1) Layering contract

Rate limiting must live above backends:

* Backends (`vfs/mem`, `vfs/host`, `vfs/overlay`) should not re-implement throttling.
* VFS core dispatch enforces throttling *uniformly* across operations and providers.

### 2) Performance is critical

* The “hot path” for a throttled op should be:

  * a couple of integer ops + one fast atomic/lock attempt when uncontended
  * and **zero cost** when rate limiting is disabled.

### 3) Sync + async required

* Async: wait using runtime hooks (no tokio hard dependency).
* Sync: block or fail fast depending on “nonblocking” semantics.

### 4) Works with mounts, overlays, and OFDs

* Mount traversal is inode-driven: limiter needs the **MountId** and **Fs instance identity** available in dispatch.
* Overlay: reads may hit lower, writes hit upper — policy must be able to apply to:

  * the overlay mount as a whole, and/or
  * the upper Fs specifically.

---

## Public API Design (vfs-ratelimit)

### Core concepts

#### 1) Operation cost model

We need a consistent definition of “what is an I/O” and “what counts as bytes”.

Create an enum used by VFS core:

```rust
/// A normalized classification for throttling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IoClass {
    /// Metadata operations (lookup, stat, chmod, utimens, etc.)
    Meta,
    /// Directory listing / enumeration.
    ReadDir,
    /// File reads (bytes count is meaningful).
    Read,
    /// File writes (bytes count is meaningful).
    Write,
    /// Open/close-like operations (optional; can map to Meta).
    OpenClose,
}
```

A “cost” is:

```rust
#[derive(Clone, Copy, Debug)]
pub struct IoCost {
    pub class: IoClass,
    /// “1” means one logical operation (for IOPS throttles).
    pub ops: u32,
    /// Bytes for bandwidth throttles. 0 for pure metadata ops.
    pub bytes: u64,
}
```

**Rules for VFS integration:**

* `lookup`, `stat`, `unlink`, `rename`, `mkdir`, `rmdir`, `readlink`, `symlink`, etc. => `Meta` with `ops=1, bytes=0`
* `readdir/getdents` => `ReadDir` with `ops=1, bytes=estimated_encoded_bytes` or `0` (see below)
* `read` => `Read` with `ops=1, bytes=requested_len` (or actual bytes read if you have it cheaply)
* `write` => `Write` with `ops=1, bytes=requested_len`
* `fsync/flush` => usually `Meta` (ops=1)

**About `readdir` bytes:**

* If core already computes an encoded buffer size (e.g. getdents), use that.
* Otherwise, don’t force expensive size computations. Use `bytes=0` and rely on IOPS for `ReadDir`.

#### 2) Limiter scopes and composition

We want to enforce multiple limits simultaneously:

* Global context limiter
* Mount limiter
* Fs limiter

Enforcement is the intersection: **all must grant capacity**.

Define:

```rust
pub struct LimiterChain {
    pub global: Option<Arc<dyn RateLimiter>>,
    pub mount: Option<Arc<dyn RateLimiter>>,
    pub fs: Option<Arc<dyn RateLimiter>>,
}
```

VFS dispatch computes the chain for each op and calls `acquire` against each present limiter.

**Order:** global → mount → fs (consistent ordering reduces deadlock risk if any locking exists).

#### 3) Blocking vs non-blocking behavior

We must match the plan requirement:

* If caller requested non-blocking semantics, return `EAGAIN`/`EWOULDBLOCK`.
* Otherwise apply back-pressure (wait).

We need a VFS-facing flag:

```rust
pub struct AcquireOptions {
    /// If true: do not wait. Return WouldBlock if insufficient capacity.
    pub nonblocking: bool,
    /// Optional: bounded wait. If exceeded, return TimedOut (maps to EAGAIN or ETIMEDOUT depending on policy).
    pub timeout: Option<Duration>,
    /// Used for fairness grouping (e.g., per-fd, per-uid, per-task).
    pub key: Option<LimiterKey>,
}
```

Where:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LimiterKey {
    /// Per-open-handle fairness
    Handle(u64),
    /// Per-process or “thread group”
    Context(u64),
    /// Optional multi-tenant fairness
    Uid(u32),
    /// Per-mount fairness
    Mount(u32),
    /// Free-form
    Named(Arc<str>),
}
```

---

## Traits

### `RateLimiter` trait (sync + async)

Keep it object-safe and runtime-agnostic.

```rust
pub trait RateLimiter: Send + Sync {
    /// Fast path: try to take capacity immediately.
    /// Returns Ok(()) on success; Err(WouldBlock) if insufficient.
    fn try_acquire(&self, cost: IoCost, opts: &AcquireOptions) -> AcquireResult;

    /// Slow path: wait until capacity is available or timeout/cancel occurs.
    /// Default implementation can loop calling try_acquire + sleep via hooks.
    fn acquire_blocking(&self, cost: IoCost, opts: &AcquireOptions, rt: &dyn SyncWait) -> AcquireResult;

    /// Async wait variant using runtime hooks.
    fn acquire_async<'a>(
        &'a self,
        cost: IoCost,
        opts: AcquireOptions,
        rt: &'a dyn AsyncWait,
    ) -> core::pin::Pin<Box<dyn core::future::Future<Output = AcquireResult> + Send + 'a>>;
}
```

Return type:

```rust
pub type AcquireResult = Result<(), AcquireError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireError {
    WouldBlock,
    TimedOut,
    Cancelled,
    Misconfigured, // e.g. zero rates with nonzero cost
}
```

**Important integration mapping:**

* `WouldBlock` → `VfsErrorKind::WouldBlock` (maps to WASI EAGAIN)
* `TimedOut` → also `WouldBlock` by default (unless you choose to map to ETIMEDOUT; simplest is EAGAIN)
* `Cancelled` → `VfsErrorKind::Interrupted` (or WouldBlock; pick one mapping and document it)
* `Misconfigured` → `InvalidInput` or `NotSupported` (prefer `InvalidInput`)

### Runtime hooks (no tokio in core)

Define in `vfs-ratelimit`:

```rust
pub trait SyncWait: Send + Sync {
    fn sleep(&self, dur: Duration);
    fn now(&self) -> Instant;
}

pub trait AsyncWait: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep<'a>(&'a self, dur: Duration)
        -> core::pin::Pin<Box<dyn core::future::Future<Output = ()> + Send + 'a>>;
    fn is_cancelled(&self) -> bool;
}
```

Then `vfs/rt` can implement these using the actual runtime used by Wasix (tokio, async-std, etc.) without coupling `vfs-ratelimit` to it.

---

## Configuration types

### Rate limits

Support both IOPS and bandwidth. Make it explicit and composable.

```rust
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Optional IOPS: operations per second.
    pub iops: Option<Rate>,
    /// Optional read bandwidth bytes/sec.
    pub read_bps: Option<Rate>,
    /// Optional write bandwidth bytes/sec.
    pub write_bps: Option<Rate>,
    /// Optional meta iops (can share iops if omitted).
    pub meta_iops: Option<Rate>,
    /// Burst controls (tokens can accumulate up to burst).
    pub burst: BurstConfig,
    /// Fairness (optional).
    pub fairness: FairnessConfig,
}

#[derive(Clone, Copy, Debug)]
pub struct Rate {
    pub per_sec: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct BurstConfig {
    /// Max extra ops that can accumulate beyond steady rate.
    pub ops_burst: u32,
    /// Max extra bytes that can accumulate beyond steady rate.
    pub bytes_burst: u64,
}

#[derive(Clone, Debug)]
pub enum FairnessConfig {
    None,
    /// Round-robin across keys (handle/context/uid).
    PerKeyRoundRobin { max_keys: usize },
    /// Weighted fairness; keys have weights provided externally.
    Weighted { max_keys: usize },
}
```

**Zero-cost when disabled:**

* In `vfs/core`, the limiter references are `Option<Arc<dyn RateLimiter>>`.
* If all are `None`, the dispatch does no work.
* Keep config parsing and limiter object creation out of the hot path.

---

## Implementation: Token Bucket (primary limiter)

### Overview

Implement a standard token bucket with:

* separate buckets for:

  * ops tokens (IOPS)
  * read bytes tokens
  * write bytes tokens
  * optional meta ops tokens
* burst capacity (max tokens)
* refill based on elapsed time from a monotonic clock (`Instant`)

### Data structure & locking

#### Requirements:

* Very cheap uncontended path.
* Correct under concurrency.
* Avoid global contention if fairness enabled.

#### Suggested implementation approach (simple + safe for juniors):

* Use a `parking_lot::Mutex` (or std `Mutex` if you prefer fewer deps) guarding the bucket state.
* Optimize by keeping `try_acquire` short and doing one lock acquisition.

Bucket state:

```rust
struct BucketState {
    last: Instant,
    ops_tokens: f64,
    meta_ops_tokens: f64,
    read_tokens: f64,
    write_tokens: f64,
}
```

Use `f64` tokens to avoid integer rounding issues when refilling at sub-second granularity. Convert costs to `f64` for comparisons.

**Refill logic:**

* `elapsed = now - last`
* `ops_tokens = min(max_ops, ops_tokens + elapsed_secs * iops_rate)`
* similarly for others
* update `last = now`

**Acquire logic:**

* Decide which buckets are relevant based on `IoCost.class`:

  * `Read` consumes ops + read bytes
  * `Write` consumes ops + write bytes
  * `Meta` consumes meta_ops or ops
  * `ReadDir` uses ops + optional bytes if provided
* If any required bucket would go negative:

  * return `WouldBlock`
* Else decrement and return `Ok(())`

### Wait strategy (blocking and async)

#### `acquire_blocking`

Pseudo:

1. First call `try_acquire`. If ok → return.
2. If `nonblocking` → return WouldBlock.
3. Compute “time until next token(s)”:

   * For each required bucket, compute:

     * missing = required - available
     * wait = missing / rate_per_sec
   * Take max across required buckets.
   * Clamp to a minimum sleep (e.g. 250µs) and a maximum sleep (e.g. 50ms) to avoid busy loops and oversleep.
4. If timeout specified, ensure we don’t exceed it.
5. `rt.sleep(wait)`
6. Loop until acquired or timeout.

#### `acquire_async`

Same logic, but use `rt.sleep().await`, and check `rt.is_cancelled()` each loop.

* If cancelled → `AcquireError::Cancelled`.

**Cancellation integration:** VFS async ops should pass an `AsyncWait` tied to the task cancellation model (e.g., drop-based cancellation is fine; `is_cancelled()` can always return false if you don’t have a signal yet).

---

## Fairness layer (optional but part of the step requirement)

We need “fairness controls (round-robin or weighted)”.

### Practical, implementable v1 fairness

Implement fairness as a wrapper limiter that:

* Groups requests by `LimiterKey`
* Limits each key’s ability to monopolize tokens

**Important:** Don’t over-engineer. We’re not building a full scheduler.

#### Approach: per-key queue with round-robin wakeups (async) + per-key turn-taking (sync)

* `FairLimiter` holds:

  * underlying `TokenBucketLimiter`
  * a map `HashMap<LimiterKey, KeyState>`
  * a round-robin list/queue of active keys
* On `acquire_async` when `WouldBlock`:

  * register the key as “waiting”
  * wait for a notification (e.g., a simple `Notify`-like abstraction provided by `vfs/rt`, or a minimal internal async wait primitive)
* On capacity refill, wake one key in round-robin order.

**But we do not have a runtime dependency.**
So: define minimal notify primitives via hooks:

* In v1, you may implement fairness only for **async** (where waiting is cheap), and for sync just do best-effort (no starvation guarantees).
* Or implement fairness purely via **turnstile**:

  * each key has a “next eligible time” computed from its share, and `try_acquire` checks it.

**Recommended v1:** implement **PerKeyRoundRobin** fairness for async only, and document sync as best-effort.

* This still satisfies “fairness controls” practically in Wasix’ async-heavy usage.

If you must support fairness for sync too:

* Use a condition variable per key and a global “current key” pointer.
* This is more code and more potential for bugs; keep it as a later enhancement unless necessary.

---

## Integration into `vfs/core`

### Where to hook

**Hook in core dispatch**, not in providers.

You want a single helper:

```rust
fn ratelimit_before_op(
    chain: &LimiterChain,
    cost: IoCost,
    opts: &AcquireOptions,
    mode: DispatchMode, // Sync/Async
) -> VfsResult<()>;
```

And call it in **every operation entry point** in VFS core that represents a filesystem-visible operation:

* path-based ops: open, stat, unlink, rename, mkdir, rmdir, symlink, readlink
* handle-based ops: read, write, seek? (seek is usually not throttled), fsync, readdir iteration

### How to obtain the LimiterChain

When dispatching an op, you should already know:

* `VfsContext` (or whatever core context object owns the mount registry)
* the current `MountId` (known in path walker resolution result)
* the resolved `Fs` instance (mounted fs)

Add optional limiter refs:

* `VfsContext.rate_limiter: Option<Arc<dyn RateLimiter>>`
* `MountEntry.rate_limiter: Option<Arc<dyn RateLimiter>>`
* `FsInstance.rate_limiter: Option<Arc<dyn RateLimiter>>`

**Where to store `FsInstance.rate_limiter`:**

* If `Fs` is a trait object, store it alongside the `Arc<dyn Fs>` in mount entry:

  * `MountEntry { fs: Arc<dyn Fs>, fs_limiter: Option<Arc<dyn RateLimiter>>, ... }`
* Or store limiter inside an `FsWrapper` struct in core that implements `Fs` and delegates.

### No/zero cost flow

Ensure in dispatch:

* If all three are `None`, do nothing.
* Avoid allocating `LimiterChain` in hot path; pass references:

  * `global.as_deref()`, etc.

### Interaction with OFD / handles

Reads and writes typically happen on `VfsHandle`.
`VfsHandle::read/write` should:

* determine its mount/fs association (store `MountId` + maybe `Arc<dyn RateLimiter>` in handle at open time)
* apply limiter before calling backend handle ops

This prevents needing to resolve mount on every read/write.

**At open time:**

* When `PathWalker` resolves and VFS creates `VfsHandle`, attach:

  * `mount_id`
  * `fs_limiter`
  * `mount_limiter`
  * `global_limiter` (or a pointer back to context)
* Keep these as `Option<Arc<dyn RateLimiter>>`.

### Interaction with mounts and `..` boundary rules

Rate limiter doesn’t change path semantics. It only gates operations once the operation is determined. However:

* For path operations that traverse multiple components, do **not** rate-limit per component unless explicitly desired.

  * Rate-limit once per syscall-like operation, not per internal lookup step.
* Exception: if path walker does heavy backend lookups and you want to prevent path-walk abuse, add a separate internal “lookup budget” limiter later. Don’t mix this into v1.

---

## Interaction with Wasix (Phase 5)

### Non-blocking semantics mapping

Wasix sets `O_NONBLOCK` (per-FD) and also has WASI flags for nonblocking reads/writes in some contexts.

Rules:

* If the **operation is nonblocking**, pass `AcquireOptions { nonblocking: true }`.
* Then limiter returns `WouldBlock` → VFS maps to `EAGAIN`.

Where to decide nonblocking:

* Wasix resource table stores per-FD flags.
* When calling into VFS for `read`/`write`, Wasix passes a flag or VFS reads it from handle state (either is fine, but don’t duplicate per-FD flags inside OFD).

### Errno mapping

Update `vfs/core::VfsErrorKind` if needed:

* `WouldBlock`
* `Interrupted` (optional)
  Then `vfs/unix::errno` maps:
* `WouldBlock` → `EAGAIN` (and `EWOULDBLOCK` if WASI distinguishes; usually EAGAIN is enough)

---

## Interaction with limits/quotas (Step 3.6)

These are orthogonal:

* **Quotas/limits**: fail when exceeding capacity (e.g., ENOSPC)
* **Rate limiting**: delay (or EAGAIN if nonblocking)

Ordering recommendation in dispatch:

1. Wasix rights (cheap, deterministic)
2. Rate limiting acquire (cheap if enabled)
3. VFS permission/policy checks (might touch metadata)
4. Backend operation
5. Quota/limit accounting for allocations (must happen where allocations occur)

But for writes that allocate:

* You may want to do quota checks *before* waiting a long time, to fail fast if impossible.
* Practical v1 rule:

  * For `write`: do rate limit first (so attackers can’t bypass rate limiting by forcing quota checks), then quota enforcement in backend/memfs.
  * This is acceptable as long as quota checks are still correct.

Document this order in `vfs/core` once implemented.

---

## Interaction with overlay (Step 3.4)

Overlay can have:

* Mount-level limiter (applies to the merged view)
* Fs-level limiters for upper and lower layers (if configured)

Rules:

* If operation is against overlay mount, apply:

  * global limiter
  * overlay mount limiter
  * overlay fs limiter (if you treat overlay as an Fs instance)
* If overlay internally performs multiple backend ops (e.g. copy-up):

  * Do **not** re-apply the mount limiter per internal op (would multiply throttle).
  * Apply a separate internal accounting strategy:

    * The outer operation consumes cost based on user-visible request size.
    * Internal copy-up can optionally consume extra bandwidth tokens from the **fs limiter of the upper** (optional enhancement).
* For v1 simplicity:

  * throttle only the user-visible op once.
  * document that internal amplification (copy-up) is not separately throttled.

---

## Test Plan & Acceptance Criteria

### Unit tests in `vfs-ratelimit`

1. **Token refill correctness**

   * Start with empty tokens, advance clock, ensure tokens refill up to burst cap.
2. **IOPS throttling**

   * Configure iops=10/sec, burst=0, attempt 20 ops immediately:

     * first ~10 succeed (depending on initial tokens), rest WouldBlock.
3. **Bandwidth throttling**

   * read_bps=1000, burst=0, try acquire 2000 bytes:

     * should WouldBlock unless you wait.
4. **Nonblocking behavior**

   * `nonblocking=true` returns WouldBlock immediately.
5. **Timeout behavior**

   * With tiny rate and short timeout, ensure TimedOut triggers.

To test without sleeping:

* Implement a `MockTime` + `MockWait` in tests to simulate time progression.

### Integration tests (suggested placement: `vfs/core/tests/ratelimit.rs`)

* Build a memfs mount with limiter:

  * Configure `read_bps` low.
  * Write a file, then read it in chunks:

    * In blocking mode, reads complete but take simulated time (use mock wait if you can hook it).
    * In nonblocking mode, repeated reads return EAGAIN until enough time passes.

### Phase 3 acceptance criteria additions for 3.7

* `cargo check -p vfs-ratelimit` passes.
* `cargo test -p vfs-ratelimit` passes.
* `vfs-core` compiles with optional ratelimit integration behind a feature flag if needed (recommended: always compile, but limiter is optional at runtime).
* A memfs-based integration test demonstrates `EAGAIN` on nonblocking throttled read/write.

---

## Crate wiring & feature flags

### Workspace

Add `vfs/ratelimit` to workspace members.

### Dependencies

Keep dependencies minimal:

* `parking_lot` (optional but recommended for performance)
* `hashbrown` (optional, only if fairness map needs it)
* No async runtime dependency.

### Optional feature: `fair`

If fairness gets complex, gate it:

* default includes token bucket only
* `features = ["fair"]` enables fairness wrapper

---

## Step 3.7 “Update the plan” note

When implementing, update the plan with:

* whether fairness shipped in v1 (async-only vs sync+async)
* which operations are throttled (read/write/meta/readdir/open)
* exact error mapping chosen (`WouldBlock` → EAGAIN)
* whether overlay copy-up internal bandwidth is additionally throttled (likely “no” for v1)

---

## Implementation Notes (2026-01-31)

- Fairness shipped as best-effort per-key round-robin (sync + async), with stale key cleanup.
- Throttled ops: handle-level read/write/pread/pwrite (metadata ops not wired yet).
- Error mapping: WouldBlock/TimedOut -> EAGAIN, Cancelled -> EINTR, Misconfigured -> EINVAL.
- Overlay copy-up amplification is not additionally throttled in v1.

---

## Implementation checklist for a junior engineer

1. Create `vfs/ratelimit` crate + modules listed above.
2. Implement `IoClass`, `IoCost`, `AcquireOptions`, `AcquireError`.
3. Implement `SyncWait` + `AsyncWait` traits.
4. Implement `TokenBucketLimiter`:

   * config parsing
   * bucket refill + try_acquire
   * acquire_blocking loop
   * acquire_async loop
5. Add mock time/wait for tests and cover refill + throttling behaviors.
6. Add minimal integration points in `vfs/core`:

   * store optional limiter refs in context/mount/fs/handle
   * call limiter before read/write and a few metadata ops
7. Add one memfs integration test verifying `EAGAIN` for nonblocking reads/writes under throttle.

If you want, I can also draft the exact `vfs/core` integration patch plan (which structs to extend and where to call the limiter) once you share the current state of the core dispatch API.
