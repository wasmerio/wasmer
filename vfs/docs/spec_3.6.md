IMPORTANT NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan!!!

## Step 3.6 Spec — Filesystem Usage Limits and Quotas (`vfs/core/src/limits.rs`)

### What Step 3.6 must accomplish

Implement **optional** usage limits and quotas for the VFS in a way that:

1. Has **zero/near-zero overhead** when limits are not configured.
2. Can enforce limits:

   * **Per-Fs instance** (e.g., a memfs mount capped to 64 MiB)
   * **Per-mount** (a mountpoint imposes additional caps/policy)
   * **Optional per-uid/gid quotas** (multi-tenant environments)
3. Works correctly with **overlay mounts**, where the writable **upper** layer must not grow without bound.
4. Supports **shared accounting** across multiple Fs instances/mounts (a “domain” that several mounts charge against).
5. Integrates cleanly with:

   * Path resolution (Phase 2.1 / 3.3)
   * Mount table (Phase 3.1)
   * Node operations (Phase 2.3)
   * Handles + OFD semantics (Phase 2.4)
   * Error mapping (Phase 1.4 / `vfs/unix/errno.rs`)
6. Is **capability-aware** and honest (don’t pretend you can enforce what you can’t measure precisely).

---

# 1) Scope and semantics

### Limits covered (all optional)

A single configured mount/Fs may set any combination of:

* `max_used_bytes`: maximum accounted bytes used
* `max_inodes`: maximum accounted inode-like objects (files, dirs, symlinks)
* `max_dir_entries`: maximum entries *in a directory* (practical “fan-out” cap)
* `max_path_length`: maximum path length (enforced during resolution)

Additionally:

* Optional `per_uid` and/or `per_gid` quotas for `max_used_bytes` and `max_inodes`.

### Definitions and accounting unit choices

* **Bytes**: accounted as logical file size usage for backends that can report size reliably (memfs, hostfs). For operations like `write`, we charge based on actual bytes added to file length (delta), not raw bytes written, to avoid double-counting overwrites.
* **Inodes**: count every created filesystem object visible in the VFS namespace (regular file, dir, symlink). Hardlinks do **not** consume a new inode (but do consume a directory entry).
* **Directory entries**: per-directory count (number of child names). This is enforced on `create`, `link`, `symlink`, `mkdir`, and `rename` that adds a new name to a target directory.
* **Path length**: enforced by `PathWalker` based on the input/normalized path string length (bytes), before performing expensive traversal.

### Error semantics

Use Linux/POSIX-style error meaning:

* Exceeding bytes/inodes: typically `ENOSPC` (no space) or `EDQUOT` (quota exceeded).
* Exceeding per-directory entries: `ENOSPC` is common; you may also add a dedicated `VfsErrorKind::DirectoryFull` if you prefer.
* Path length exceeded: `ENAMETOOLONG`.

**Implementation rule:** in `vfs/core`, return a **semantic** `VfsErrorKind` (e.g., `NoSpace`, `QuotaExceeded`, `NameTooLong`); the exact WASI errno mapping is centralized in `vfs/unix`.

---

# 2) “Zero cost when disabled” design

### Requirement

If no limits/quotas are configured:

* No allocations
* No lock acquisition
* No per-op hashing
* Ideally just one predictable branch on a null/None pointer

### Mechanism

The VFS should carry an `Option<Arc<LimitsController>>` (or `Option<Arc<dyn Limits>>`) at the mount/Fs dispatch level:

* `MountEntry` holds `limits: LimitsRef` where:

  * `type LimitsRef = Option<Arc<LimitsController>>;`
* If `limits.is_none()`, VFS operations **must not** call into any accounting code.

This becomes the fast-path:

```rust
if let Some(limits) = mount.limits.as_ref() {
    limits.preflight_…();
}
```

---

# 3) Public API and type design (`vfs/core/src/limits.rs`)

## 3.1 Core configuration types

### `LimitsConfig`

A plain configuration struct that can be placed in:

* Fs provider config (Fs defaults)
* Mount options (mount-specific overrides)
* Overlay options (upper enforcement)

```rust
#[derive(Clone, Debug, Default)]
pub struct LimitsConfig {
    pub max_used_bytes: Option<u64>,
    pub max_inodes: Option<u64>,
    pub max_path_length: Option<u32>,
    pub max_dir_entries: Option<u32>, // per-directory

    pub per_uid: Option<QuotaTableConfig>,
    pub per_gid: Option<QuotaTableConfig>,

    // Optional: if set, charges are applied to a shared accounting domain
    pub accounting_domain: Option<AccountingDomainId>,
}
```

### `QuotaTableConfig`

Defines per-uid/gid quota caps (and default fallback).

```rust
#[derive(Clone, Debug)]
pub struct QuotaTableConfig {
    pub default: Quota,                 // default quota for ids not listed
    pub entries: Vec<(u32, Quota)>,      // uid/gid -> Quota
}

#[derive(Clone, Debug, Default)]
pub struct Quota {
    pub max_used_bytes: Option<u64>,
    pub max_inodes: Option<u64>,
}
```

### `AccountingDomainId`

A small identifier allowing shared accounting across mounts.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AccountingDomainId(pub u64);
```

> Integration note: the provider registry/mount API should allow callers (Wasix env builder, tests) to wire multiple mounts to the same `AccountingDomainId`.

---

## 3.2 Runtime controller types

### `LimitsController`

Responsible for:

* Combining effective caps from Fs defaults + mount overrides
* Tracking usage (global + per uid/gid)
* Supporting reservation/commit flows for writes/truncates

Key property: **small, cheap reads**; updates may lock but only when enabled.

```rust
pub struct LimitsController {
    effective: EffectiveLimits,
    accounting: Arc<dyn AccountingBackend>,
}
```

### `EffectiveLimits`

A precomputed structure (no maps) for the common checks.

```rust
#[derive(Clone, Debug)]
pub struct EffectiveLimits {
    pub max_used_bytes: Option<u64>,
    pub max_inodes: Option<u64>,
    pub max_path_length: Option<u32>,
    pub max_dir_entries: Option<u32>,

    pub per_uid: Option<QuotaTable>,
    pub per_gid: Option<QuotaTable>,
}
```

`QuotaTable` should be a structure optimized for read-mostly:

* store a small sorted vec + binary search, or a hash map behind an `Arc`.
* because quotas are optional, it’s fine if quota lookups are a bit heavier; they shouldn’t exist in the common “no limits” case.

### `AccountingBackend` trait (the “shared accounting hook”)

This is the key extension point for “shared accounting across multiple Fs instances, or even across domains”.

```rust
pub trait AccountingBackend: Send + Sync {
    fn usage_global(&self) -> UsageSnapshot;

    fn try_reserve(&self, req: ChargeRequest) -> Result<Reservation, VfsError>;
    fn commit(&self, res: Reservation, actual: ChargeActual);
    fn rollback(&self, res: Reservation);
}
```

* A default in-crate implementation is required (see below).
* External callers (Wasix runtime, embedding app) can provide their own backend to unify filesystem + network + other resources.

### Reservation model

Writes/truncates are the hardest part because actual growth is only known after the backend acts.

```rust
#[derive(Clone, Debug)]
pub struct ChargeRequest {
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub bytes: u64,    // “worst case” growth request
    pub inodes: u64,
}

#[derive(Clone, Debug)]
pub struct Reservation { /* opaque token */ }

#[derive(Clone, Debug)]
pub struct ChargeActual {
    pub bytes: u64,    // actual growth
    pub inodes: u64,
}
```

Rules:

* Preflight: `try_reserve(request)` must fail if this would exceed any configured cap.
* Commit: `commit(res, actual)` charges only actual, and releases any unused reservation.
* Rollback: used when backend op fails.

This allows correctness without double-counting.

---

## 3.3 Default accounting backend implementation

Provide `InMemoryAccountingBackend` in `limits.rs` (or a small `limits/accounting.rs` submodule) that supports:

* Global usage counters (AtomicU64)
* Optional per-uid/gid usage maps (behind a lock)

Suggested structure:

```rust
pub struct InMemoryAccountingBackend {
    global_bytes: AtomicU64,
    global_inodes: AtomicU64,

    // Allocated only if per_uid/per_gid is configured:
    per_uid: Option<parking_lot::RwLock<HashMap<u32, Usage>>>,
    per_gid: Option<parking_lot::RwLock<HashMap<u32, Usage>>>,
}
```

Where:

```rust
#[derive(Clone, Copy, Debug, Default)]
pub struct Usage { pub bytes: u64, pub inodes: u64 }
```

### Concurrency behavior

* `try_reserve` must be race-safe.
* Easiest correct approach (good enough for v1):

  * Lock the necessary maps (if configured)
  * Check limits
  * Apply reservation in the same critical section
* Because limits are optional and expected to be off by default, it’s acceptable if the “limits enabled” path uses locks.

If you want lock-free global counters:

* global checks can be done with `compare_exchange` loops on atomics.
* per-uid/gid still likely needs locks.

---

# 4) Integration points across the plan

## 4.1 Mount table integration (Phase 3.1)

### Where limits live

* `MountEntry` should carry:

  * `mount_limits: LimitsRef`
  * Additionally, the `Fs` instance may carry default limits (from provider config).
* The effective controller for a mount should be the merge:

  * `effective_limits = merge(fs_defaults, mount_overrides)`

**Merge rule:** mount overrides win if set; otherwise inherit Fs defaults.

### Shared accounting domains

Mount API should accept either:

* A concrete `Arc<dyn AccountingBackend>` (advanced)
* Or an `AccountingDomainId` that resolves to a shared backend stored in a VFS context object.

This enables:

* Multiple mounts share the same pool (“all scratch space in this sandbox is 200 MiB total”)
* Overlay upper + a separate tmp mount share the same pool

## 4.2 PathWalker integration (Phase 2.1 / 3.3)

Path length enforcement belongs in `PathWalker` because it’s cheap and avoids backend work.

* Add a hook in resolution entrypoints:

  * Determine the effective `max_path_length` based on the current mount (or global VFS defaults).
  * Check the incoming path length in bytes.
  * If exceeded, return `VfsErrorKind::NameTooLong`.

Notes:

* For relative resolution across mounts: path length applies to the user-provided path string, not per-component.
* If you normalize paths into a scratch buffer, check both the original and normalized if your normalization can expand (it usually shouldn’t).

## 4.3 Node operation integration (Phase 2.3)

Limits must be enforced where namespace mutations happen:

### Inode-consuming operations (reserve/commit inodes)

* `create` (O_CREAT new file)
* `mkdir`
* `symlink`
* “copy-up create” in overlay upper

### Directory entry consuming operations

* `create` (adds name)
* `mkdir` (adds name)
* `symlink` (adds name)
* `link` (adds name)
* `rename` (adds name to target dir if target name did not exist; handle replacement carefully)

For `max_dir_entries` (per-directory):

* This is **not global accounting**; it’s a property of a specific directory.
* Enforcement point is `lookup/create` on a parent directory node:

  * Before adding a new name, check the directory’s current child count.
  * Backends:

    * memfs can track child count exactly
    * hostfs may need to `readdir`/count (expensive) → best-effort or disabled unless hostfs tracks it
* Recommendation:

  * For v1, enforce `max_dir_entries` precisely in memfs and overlay (upper memfs), and treat it as **best-effort** for hostfs unless you later add a cached child-count index.

## 4.4 Handle and OFD integration (Phase 2.4)

### Writes/truncates must enforce byte limits

The correct place is the VFS handle methods (because they own the “semantic write” call):

* `write` / `write_at`:

  1. Determine worst-case growth request.

     * If writing at offset beyond EOF, worst case growth is `offset + len - old_len`.
     * If overwriting within existing length, growth may be 0.
  2. `try_reserve(bytes = worst_case_growth)`
  3. Call backend write
  4. Compute actual growth by comparing new length vs old length
  5. `commit(reservation, actual_growth)`

* `truncate`:

  * Shrinks: should **release** accounted bytes
  * Grows: reserve growth then commit

This requires you to know file length before/after. For memfs that’s trivial. For hostfs/object stores:

* If precise size tracking is hard/expensive, you can:

  * Maintain a VFS-level cached size for open handles (updated after writes/truncates)
  * Fall back to `stat` for before/after when necessary
* Be explicit: if a backend cannot provide reliable size deltas, document that byte accounting is best-effort.

## 4.5 Overlay integration (Phase 3.4)

Overlay must enforce limits primarily in the **upper** layer.

Key rules:

* Copy-up that creates a new upper file must reserve:

  * 1 inode (if new)
  * bytes equal to the copied file’s data length (unless you implement metacopy later)
* Writes to a copied-up file follow normal handle write enforcement against the **upper’s** limits/accounting domain.

“Global accounting across overlay layers” in this plan means:

* If you configure an accounting domain for the overlay mount, **charge everything to that domain**, regardless of whether an object is newly created in upper or already existed in upper.
* Lower layers are read-only and should not be charged for growth caused by overlay operations (since they don’t grow), but their existing size may or may not count depending on policy.

  * Recommended v1 semantics: **only charge writable growth** (upper usage), not pre-existing lower content. This matches the “prevent unbounded in-memory growth” requirement and avoids huge “baseline charges” from large base images.

Document this choice in overlay docs/tests.

---

# 5) Provider configuration and mount flags exposure

## 5.1 Provider config

Each provider’s mount config struct should optionally include limits. Example:

```rust
pub struct MemFsConfig {
    pub limits: Option<LimitsConfig>,
    // ...
}
```

Providers that can enforce precisely (memfs) should do so and keep metadata needed (child counts, file sizes, inode counts).
Providers that can’t should:

* still accept the config
* enforce what they can
* return `NotSupported` or best-effort behavior as documented

## 5.2 Mount API

Mount options should allow overriding limits:

```rust
pub struct MountOptions {
    pub flags: MountFlags,
    pub limits: Option<LimitsConfig>,
    // ...
}
```

Mount creates an effective `LimitsController` if either Fs defaults or mount overrides specify limits.

---

# 6) Error kinds and mapping requirements

## 6.1 Add/confirm `VfsErrorKind` variants in `vfs/core`

Ensure these exist (names flexible, but semantics required):

* `NameTooLong`
* `NoSpace` (ENOSPC)
* `QuotaExceeded` (EDQUOT) — used when a quota table triggers, even if global space is available
* `NotSupported` (for limits you can’t enforce in a backend, if you choose to hard-fail)

## 6.2 Map in `vfs/unix/src/errno.rs`

* `NameTooLong` → `ENAMETOOLONG`
* `NoSpace` → `ENOSPC`
* `QuotaExceeded` → `EDQUOT` (if WASI layer supports; otherwise map to closest supported error consistently)

---

# 7) Module structure (no huge files)

Even though deliverable mentions `limits.rs`, keep it clean:

* `vfs/core/src/limits/mod.rs`
* `vfs/core/src/limits/config.rs`
* `vfs/core/src/limits/controller.rs`
* `vfs/core/src/limits/accounting.rs`
* `vfs/core/src/limits/error.rs` (optional; or in core error module)

Then `vfs/core/src/limits.rs` can be a small re-export shim if you prefer the plan’s path literally.

---

# 8) Test plan and acceptance criteria

### Unit tests (`vfs-core`)

1. **No limits configured**

   * Create/write files in memfs works as before.
   * No new errors.

2. **Max bytes**

   * Configure memfs with `max_used_bytes = 10`.
   * Write 10 bytes succeeds; writing 1 more returns `NoSpace` (maps to ENOSPC).
   * Truncate down releases bytes, allowing new writes.

3. **Max inodes**

   * Configure `max_inodes = 2`.
   * Create two files succeeds; third fails with `NoSpace` or `QuotaExceeded` (depending on which limit triggers).

4. **Per-uid quota**

   * Configure per-uid quota for uid=1000 to `max_used_bytes=8`.
   * Charge operations with uid=1000 fail after 8 bytes.
   * Charge operations with uid=1001 unaffected (uses default quota).

5. **Max path length (PathWalker)**

   * Configure mount `max_path_length = 5`.
   * Resolving `"abcdef"` returns `NameTooLong`.

6. **Overlay upper enforcement**

   * Overlay with upper memfs limit 16 bytes.
   * Copy-up a 12-byte file succeeds; further growth beyond 16 fails.

### Phase 3 acceptance criteria additions (for Step 3.6 specifically)

* All tests above pass with:

  * `cargo test -p vfs-core`
  * `cargo test -p vfs-mem` (if memfs integrates enforcement internally)
* Limits are **not** required for normal operation; disabling them changes no behavior.

---

# 9) Implementation checklist (junior-friendly)

1. Create `limits/` module structure and re-exports.
2. Implement `LimitsConfig`, `QuotaTableConfig`, `Quota`, `AccountingDomainId`.
3. Implement `AccountingBackend` + default `InMemoryAccountingBackend`.
4. Implement `LimitsController`:

   * Build `EffectiveLimits` by merging Fs defaults + mount overrides.
   * Provide helpers:

     * `check_path_len(path_len)`
     * `try_reserve_bytes(uid,gid,bytes)`
     * `try_reserve_inode(uid,gid)`
     * `release_bytes(...)` (for truncates)
5. Wire into mount creation:

   * If no limits anywhere → `mount.limits = None`
   * Else create controller with either shared domain backend or per-mount backend
6. Wire into `PathWalker` for `max_path_length`.
7. Wire into VFS op dispatch:

   * Create/mkdir/symlink/link/rename (inodes/dir-entries)
   * Handle-level write/truncate (bytes)
8. Add/confirm error kinds; update errno mapping in `vfs/unix`.
9. Add tests.

---

If you want, I can also include a small “integration diff guide” for where the calls should sit in `PathWalker`, `mount.rs`, `node.rs`, and `handle.rs` based on the stubs you already have—but the above is the full Step 3.6 implementation spec.
