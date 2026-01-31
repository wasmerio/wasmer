IMPORTANT NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan!!!

## Dcache Spec — Directory Lookup Cache for `vfs/core`

### Goal (what this spec must deliver)

Add a **directory/name lookup cache (dcache)** to `vfs/core` so repeated path resolution can skip
redundant backend `lookup(parent, name)` calls.

Concretely:

- The hot loop in `vfs/core/src/path_walker.rs` currently does a backend lookup per component:
  `current.node().lookup(&name_ref)?` (sync) / `.lookup(&name_ref).await?` (async).
- With dcache enabled, we cache the result of:

  \[
  (\text{mount}, \text{parent backend inode}, \text{name}) \rightarrow \text{child node}
  \]

  and use it to short-circuit the backend lookup.

This spec is based on `vfs/docs/dcache-proposal.md`, but it is **polished to match the current
implementation** (notably:
sync vs async node types, and `MountId` reuse on unmount).

---

## Non-goals (v1)

- No readdir/listing cache (separate concern; can be added later).
- No cross-process shared dcache.
- No “global inode cache” (we cache lookup results, not inode→node).
- **Negative caching is optional and deferred**: v1 should ship with **positive caching only**,
  to keep invalidation small and correct.

---

## Why this is safe and useful (in this codebase)

### Where lookups happen today

`PathWalker::{resolve_internal, resolve_internal_async}` is the single traversal loop for most VFS
operations (see `vfs/core/src/path_walker.rs`). Each `WorkComponent::Normal(name)` performs:

- name validation (length + `VfsName` validation)
- permissions check via policy (metadata + traverse check)
- `lookup(name)` on the backend node
- mountpoint transition check via `MountTable::enter_if_mountpoint(...)`
- symlink handling (optional follow + injected work queue)

The dcache integrates at the **lookup** step only; it must **not** change mountpoint logic,
symlink handling, or security checks.

### Correctness constraints

The dcache must not return stale answers. In this VFS, staleness can be introduced by:

- VFS mutations (create/mkdir/unlink/rename/symlink/link/rmdir) that change a directory namespace
- External mutations (filesystem changes that occur outside this VFS instance)
- `MountId` reuse after unmount (a correctness bug if cache entries survive reuse)

This spec addresses these with:

- Explicit invalidation after successful mutations (in `vfs/core/src/vfs.rs`)
- An opt-in “dcache-safe” capability on `FsSync/FsAsync` instances
- Clearing all cache entries for a `MountId` when that mount id is allocated/reused/freed

---

## Terminology

- **Positive entry**: a cached successful lookup; “this name exists and maps to this node”.
- **MountId**: VFS mount identifier (`crate::MountId`).
  Important: in `vfs/core/src/mount.rs`, mount ids **can be reused** (freed slot reused by a later
  mount). The cache must treat `MountId` as stable only for the lifetime of that mount entry.
- **BackendInodeId**: backend inode id within a filesystem instance (`crate::BackendInodeId`).
- **Name**: a single path component (`VfsName` / `VfsNameBuf`).

---

## 1) Linux-like coherence model (cacheable mounts, generations, revalidation)

Linux’s dcache supports multiple coherence strategies depending on filesystem guarantees.
To get as close as possible to that model in `vfs/core` while still supporting backends that
**cannot** be invalidated, we implement **per-mount coherence modes** and (where supported)
**generation-based revalidation**.

### 1.1 New capability bits on `VfsCapabilities`

Add the following bits to `vfs/core/src/capabilities.rs`:

- `VfsCapabilities::DCACHE_STRICT`  
  “Linux-like strict coherence”: the namespace only changes through this VFS instance.
  Cached dentries are valid until explicitly invalidated by VFS mutations.

- `VfsCapabilities::NAMESPACE_IMMUTABLE`  
  The namespace is a snapshot/immutable (e.g. packaged/embedded RO fs). Cached dentries are
  **always valid** for the lifetime of the mount; no invalidation or revalidation is required.

- `VfsCapabilities::DIR_CHANGE_TOKEN`  
  Directories can expose a cheap “change token” (generation) that changes whenever directory
  entries change. This allows Linux-style revalidation: cache hits are accepted only if the token
  matches the current directory token.

Notes:

- These are **Fs-instance** capabilities (`FsSync::capabilities()` / `FsAsync::capabilities()`),
  because `MountEntry` already stores `fs_sync` / `fs_async`. This avoids introducing provider-level
  plumbing into the mount table.

### 1.2 Optional directory generation (“change token”) hooks

To support generation-based revalidation, add defaulted methods to the node traits.
These must be object-safe and have defaults so existing implementations don’t break.

Add to `FsNodeSync` (in `vfs/core/src/traits_sync.rs`):

```rust
/// A token that changes when this directory's children set changes.
/// Only meaningful for directories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DirChangeToken(pub u64);

fn dir_change_token(&self) -> VfsResult<Option<DirChangeToken>> {
    Ok(None)
}
```

And to `FsNodeAsync` (in `vfs/core/src/traits_async.rs`) the async equivalent:

```rust
async fn dir_change_token(&self) -> VfsResult<Option<DirChangeToken>> {
    Ok(None)
}
```

Rules:

- If the fs advertises `VfsCapabilities::DIR_CHANGE_TOKEN`, it must return `Some(token)` for
  directory nodes and must update the token on any namespace change.
- If it returns `None`, `PathWalker` cannot do generation-based revalidation and must fall back
  to weaker modes (TTL/off) unless `DCACHE_STRICT` or `NAMESPACE_IMMUTABLE` applies.

### 1.3 Read-only mounts that “always remain valid”

We support the “proper kernel-like” case for read-only snapshot filesystems:

- If a mount’s `Fs` has `VfsCapabilities::NAMESPACE_IMMUTABLE`, the dcache entries for that mount
  are **always valid** (for the lifetime of that mount).
- If additionally `MountFlags::READ_ONLY` is set, the VFS never mutates it, so:
  - No invalidation hooks are needed at all.
  - Revalidation is unnecessary.

Important nuance (documented invariant):

- `MountFlags::READ_ONLY` alone does **not** prove immutability (hostfs may still change externally).
  For “always valid” caching, the fs must explicitly assert `NAMESPACE_IMMUTABLE`.

### 1.4 Per-mount coherence mode selection

At runtime, for each mount we compute a coherence mode (cheap: just flags + capability bits):

| Mode | When selected | Correctness model | What the walker does |
|------|---------------|-------------------|----------------------|
| **Immutable** | `NAMESPACE_IMMUTABLE` | Always valid | cache hit returns immediately |
| **StrictInvalidate** | `DCACHE_STRICT` (and not immutable) | Linux-like strict | cache hit returns; VFS mutations invalidate |
| **RevalidateByDirToken** | `DIR_CHANGE_TOKEN` (and not strict/immutable) | Linux-like revalidation | on hit, compare parent dir token; mismatch → miss |
| **TTL** | configured TTL > 0 | weak coherence | cache entry expires by time |
| **Disabled** | none apply | no caching | always do backend lookup |

Notes:

- “TTL” is intended as a last resort for backends that can’t invalidate and can’t expose a token.
- “StrictInvalidate” can still apply to RO mounts; it simply won’t observe invalidations because
  the VFS never mutates. (It remains correct because strict implies no external mutations.)

---

## 2) Cache keys and values

### 2.1 Key (includes a mount “epoch” generation)

`DcacheKey`:

- `mount: MountId`
- `mount_epoch: u64` (see §7; prevents MountId reuse bugs without full-cache scans)
- `parent: BackendInodeId` (the parent directory’s backend inode within that mount)
- `name: VfsNameBuf` (validated single component)

Why include `mount_epoch`?

- In `vfs/core/src/mount.rs`, mount ids can be reused after unmount.
- With a per-mount epoch (incremented on each new mount installation into that slot),
  a stale cache entry from a previous mount instance will never be addressable by a newer mount,
  even if it reuses the same `MountId`.

Why `BackendInodeId` and not `VfsInodeId`?

- `VfsInodeId` contains the mount already; storing mount twice is redundant.
- `BackendInodeId` is the stable directory identity within the mount instance.

### 2.2 Value (Linux-like: positive entries + optional metadata for revalidation)

We store **the resolved child node** (trait object) so a cache hit can skip backend calls entirely.

Additionally, to support revalidation we may store:

- `parent_dir_token: Option<DirChangeToken>` (captured at insert time, if available)
- `expires_at: Option<Instant>` (for TTL mode; `None` for strict/immutable)

Because the sync and async walkers use different node trait types, we must maintain **two caches**:

- `DcacheSync` stores `Arc<dyn crate::node::FsNode>` (alias for `FsNodeSync`)
- `DcacheAsync` stores `Arc<dyn crate::node::FsNodeAsync>`

No attempt is made to “convert” between sync and async nodes in the cache.

### 2.3 Important lifetime/ownership rule

Caching holds extra `Arc` references to nodes. This is intended and acceptable as long as:

- The cache is **bounded** (fixed max entries; eviction)
- There is no expectation that dropping all external handles immediately drops backend nodes

---

## 3) Dcache data structure and APIs

### 3.1 Code layout (new module)

Add:

- `vfs/core/src/dcache.rs`

Export:

- `pub(crate)` types for internal use (prefer not to expose publicly yet)

### 3.2 Public surface in `vfs/core` (internal API)

Define:

```rust
// vfs/core/src/dcache.rs
use crate::{BackendInodeId, MountId};
use crate::path_types::{VfsName, VfsNameBuf};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DcacheKey {
    pub mount: MountId,
    pub mount_epoch: u64,
    pub parent: BackendInodeId,
    pub name: VfsNameBuf,
}

pub(crate) struct Dcache {
    pub(crate) sync: DcacheSync,
    pub(crate) async_: DcacheAsync,
}

pub(crate) struct DcacheSync { /* bounded map */ }
pub(crate) struct DcacheAsync { /* bounded map */ }

impl Dcache {
    pub(crate) fn invalidate_one(&self, mount: MountId, parent: BackendInodeId, name: &VfsName);
    pub(crate) fn invalidate_children_of(&self, mount: MountId, parent: BackendInodeId);
    pub(crate) fn invalidate_mount(&self, mount: MountId);
}
```

### 3.3 Storage choice (fast + bounded + configurable)

Requirements:

- **Fast lookups** on the path-walk hot path.
- **Bounded space**: must not grow unbounded; must be configurable.
- **Low contention** for concurrent resolves.

Recommended design (v1):

- **Sharded LRU**: pick \(2^k\) shards; each shard holds an LRU map protected by a small lock.
- **Bound by entries and by bytes**:
  - `max_entries` total across shards (enforced per-shard as `ceil(max_entries / shards)`).
  - `max_bytes` total across shards (track per-shard byte usage; evict until under budget).

Data structures:

- `hashbrown::HashMap` (fast hash map) under the hood (via `lru` crate or directly).
- Prefer a fast hasher (`ahash`) for the key map, as these keys are internal and not user-controlled.
- Locks: `parking_lot::Mutex` per shard.

This requires adding dependencies to `vfs-core`:

- `hashbrown`
- `ahash`
- optionally `lru` (can still be used inside each shard as the LRU list+map)

### 3.4 Configuration (must be supported)

Introduce a `DcacheConfig` (owned by the dcache / mount table builder), for example:

```rust
pub struct DcacheConfig {
    pub enabled: bool,
    pub shards: usize,            // power-of-two recommended (e.g. 16)
    pub max_entries: usize,       // hard cap
    pub max_bytes: usize,         // hard cap; include key name bytes + entry overhead estimate
    pub ttl_ms: Option<u64>,      // enables TTL mode when not strict/immutable/token
}
```

Rules:

- If `enabled == false` or both caps are zero → dcache is disabled (zero overhead).
- The implementation must enforce both caps. If `max_bytes` is exceeded, evict LRU entries until
  under budget, even if `max_entries` is not hit.

### 3.5 “Zero cost when disabled”

When dcache is disabled, `PathWalker` must not touch the cache at all.

Implementation approach:

- `MountTable` stores `dcache: Option<Arc<Dcache>>`
- The hot loop checks `if let Some(dcache) = mount_table.dcache() { ... }`
- The mount eligibility check is only evaluated when dcache exists

---

## 4) Ownership: put dcache next to mount lifecycle

### 4.1 Why `MountTable` must own (or strongly reference) dcache

In `vfs/core/src/mount.rs`, mount ids can be **reused** after unmount:

- `mount_with_limiters(...)` scans for a `None` slot and reuses that index as the new `MountId`.
- `unmount(...)` can set `inner.mounts[target_mount.index()] = None` when fully removed.
- `reclaim_detached(...)` can later free a detached mount slot internally.

If dcache entries are keyed by `MountId` and survive when a `MountId` is reused, the dcache can
return nodes from a *previous* filesystem instance. That is a correctness bug.

Therefore, the code that allocates/frees/reuses mount ids must be able to clear cache entries for
that mount id.

### 4.2 Required structural change

Add to `MountTable`:

- `dcache: Option<Arc<Dcache>>`

Add a constructor that allows enabling it, without breaking existing call sites:

- Keep `MountTable::new(...)` (dcache disabled).
- Add `MountTable::new_with_config(...)` or `MountTable::new_with_dcache(...)` that initializes
  a bounded dcache.

Add a getter:

- `pub(crate) fn dcache(&self) -> Option<Arc<Dcache>>` (or `Option<&Arc<Dcache>>`)

---

## 5) Using the dcache in `PathWalker` (sync and async)

### 5.1 Integration point (sync) with coherence modes

In `vfs/core/src/path_walker.rs` inside `WorkComponent::Normal(name)`:

Current code:

- validates name
- checks dir type + traverse permission
- `let child = current.node().lookup(&name_ref)?;`

New behavior:

- **Always do validation and traverse permission checks** exactly as today.
- Only the backend `lookup(...)` call becomes conditional on cache hit/miss.

Pseudo-code (sync):

```rust
let entry = inner.mounts.get(current.mount().index()).and_then(|s| s.as_ref());
let caps = entry.map(|e| e.fs_sync.capabilities()).unwrap_or(VfsCapabilities::NONE);
let mode = dcache_mode_from_caps_and_config(caps, entry.map(|e| e.flags), dcache_config);

if matches!(mode, DcacheMode::Disabled) {
    child = current.node().lookup(&name_ref)?;
} else {
    if let Some(dcache) = self.mount_table.dcache() {
        let mount_epoch = entry.expect("mode != Disabled implies entry exists").mount_epoch;
        let parent_inode = current.node().inode();
        let parent_token = current.node().dir_change_token()?; // only used in token mode
        let key = DcacheKey { mount: current.mount(), mount_epoch, parent: parent_inode, name: name_buf };
        if let Some(entry) = dcache.sync.get(&key) {
            if dcache_entry_is_valid(mode, entry.parent_dir_token, parent_token, entry.expires_at) {
                child = entry.node.clone();
            } else {
                child = current.node().lookup(&name_ref)?;
                dcache.sync.insert(key, child.clone(), parent_token, mode);
            }
        } else {
            child = current.node().lookup(&name_ref)?;
            dcache.sync.insert(key, child.clone(), parent_token, mode);
        }
    } else {
        child = current.node().lookup(&name_ref)?;
    }
}
```

Important details:

- `name_buf` should be an owned `VfsNameBuf` for the key.
  You can build it from `name_bytes.clone()` after validation.
- The dcache must be consulted **after** the traverse permission check (to preserve policy checks).
- On a cache hit in **RevalidateByDirToken** mode:
  - Query `current.node().dir_change_token()?` (directory only).
  - Compare with the cached `parent_dir_token`.
  - If mismatch (or token missing) → treat as miss (do backend lookup and refresh).
- On a cache hit in **TTL** mode:
  - If expired → treat as miss.
  - If not expired → accept hit.

### 5.2 Integration point (async)

Mirror the sync logic in `PathWalkerAsync` with:

- `entry.fs_async.capabilities()` (same `VfsCapabilities` bits: `NAMESPACE_IMMUTABLE` /
  `DCACHE_STRICT` / `DIR_CHANGE_TOKEN`)
- `dcache.async_` store
- `lookup(&name_ref).await?` on miss

### 5.3 Do not change mountpoint / symlink behavior

After producing `child_ref`, the existing logic must remain intact:

- symlink checks and work queue injection
- `MountTable::enter_if_mountpoint(...)` transition
- stack handling for `..` and `resolve_beneath`

The cache only replaces the backend lookup.

---

## 6) Invalidation on VFS mutations (`vfs/core/src/vfs.rs`)

### 6.1 Rule: invalidate after successful mutation

After a mutation that changes a directory’s namespace, invalidate the affected lookup keys.

Key principle:

- **Only invalidate on success** (after the backend mutation returns `Ok(…)`).

### 6.2 Which VFS methods exist today (must be updated)

In `vfs/core/src/vfs.rs`, add invalidation hooks to:

- `openat` when `OpenFlags::CREATE` is used (after successful `create_file`)
- `openat_async` with `CREATE`
- `mkdirat`, `mkdirat_async` (after successful `mkdir`)
- `unlinkat`, `unlinkat_async` (after successful `unlink`)
- `renameat`, `renameat_async` (after successful `rename`)
- `symlinkat`, `symlinkat_async` (after successful `symlink`)

Note:

- `vfs.rs` currently does not expose `rmdir` or `link` operations, but the backend traits have them.
  When those VFS APIs are added, they must follow the same invalidation pattern.

### 6.3 Invalidations to perform (StrictInvalidate mode)

For **`DcacheMode::StrictInvalidate`** mounts, perform **exact invalidations**:

- create/mkdir/symlink/link: invalidate `(mount, parent_inode, name)`
  - This removes stale “old node” entries, and ensures the next lookup sees the new node.
- unlink/rmdir: invalidate `(mount, parent_inode, name)`
  - This removes the “now deleted” cached node.
- rename:
  - invalidate `(mount, old_parent_inode, old_name)`
  - invalidate `(mount, new_parent_inode, new_name)`

For other modes:

- **Immutable**: never invalidate (namespace doesn’t change).
- **RevalidateByDirToken**: invalidation is optional (correctness is provided by token mismatch).
  You may still invalidate to keep hit rates high after VFS-driven mutations.
- **TTL**: invalidation is optional (entries expire naturally).

Implementation detail:

In VFS methods, you already have:

- `ResolvedParent { dir: Resolved { mount, node, ... }, name: VfsNameBuf, ... }`

Convert `VfsNameBuf` to `VfsName` using the existing helper:

- `Vfs::name_from_buf(&parent.name)?`

Then call:

- `mount_table.dcache().map(|dc| dc.invalidate_one(mount, parent_backend_inode, &name_ref));`

### 6.4 Should invalidation be conditional?

Yes. Invalidate only if:

- dcache is enabled, and
- the mount’s selected mode is `StrictInvalidate` (or optionally also `RevalidateByDirToken`)

---

## 7) Mount lifecycle invalidation (required for correctness)

### 7.1 Mount epochs (Linux-like “superblock generation”)

To avoid correctness bugs from `MountId` reuse **without** requiring whole-cache scans on reuse,
each mount slot must have a monotonically increasing **mount epoch**.

Add to `MountEntry` (or parallel mount metadata):

- `mount_epoch: u64`

Rules:

- Every time a new mount is installed into a slot (in `mount_with_limiters`), increment the epoch.
- The dcache key includes `mount_epoch`, so a reused `MountId` never collides with old entries.

### 7.2 When to clear per-mount cache entries (space reclamation)

With mount epochs, clearing on reuse is no longer required for correctness, but it is still useful
to reclaim memory quickly.

Clear entries for `(MountId, mount_epoch)`:

- when a mount is fully removed (unmount clears the slot), and
- when a detached mount is reclaimed.

Optionally clear on mount creation as well to free any leftover entries from earlier epochs, but
correctness does not require it.

### 7.3 Where to implement (exact files)

Update `vfs/core/src/mount.rs`:

- In `mount_with_limiters`:
  - Allocate the next `mount_epoch` for `entry_id` and store it on the entry.
  - (Optional) call `invalidate_mount(entry_id)` to eagerly reclaim old entries.
- In `unmount`:
  - When the mount is actually removed (slot becomes `None`), call `invalidate_mount(target_mount)`
    (or `invalidate_mount_epoch(target_mount, removed_epoch)` if you store epoch on the key).
  - For detach mode (state set to Detached, slot not cleared), do not clear the mount cache yet.
- In `reclaim_detached`:
  - Right before `inner.mounts[mount.index()] = None;`, call `invalidate_mount(mount)` (or epoch form).

### 7.4 What does `invalidate_mount` do?

Remove all keys where `key.mount == mount` (and optionally also match epoch, if implemented).

With an LRU cache, this is easiest via a full scan. This is acceptable because:

- unmount is rare relative to lookups
- cache is bounded (size-limited)

---

## 8) Testing plan (must be implemented with the feature)

Add tests that prove:

### 8.1 Cache hit avoids repeated backend lookups

- Create a test filesystem/node that counts `lookup` calls.
- Resolve the same path twice.
- Assert that the second resolution performs fewer `lookup` calls (ideally zero for the repeated components).

Where:

- Prefer `vfs/core/tests/dcache.rs` (integration-style), or add to existing `path_walker.rs` tests.

### 8.2 Invalidation after mutation

- Resolve `dir/file` to populate the cache.
- `unlinkat` the file.
- Resolve again: should fail with `NotFound` and must not return the cached node.

Also test:

- create_file then resolve returns new node
- rename invalidates both old and new names

### 8.3 MountId reuse correctness test

This is the key regression test.

- Create mount table with dcache enabled.
- Mount a filesystem at some mountpoint, obtain `mount_id_A`.
- Perform lookups within that mounted fs to populate entries keyed by `mount_id_A`.
- Unmount it so the mount slot is freed (ensure open_count is 0).
- Mount a different fs, confirm it reuses the same mount id (likely `mount_id_A` again).
- Resolve a path in the new mount and assert it does **not** return nodes from the previous mount.

If implementing a deterministic reuse is hard in the test, mount/unmount repeatedly until reuse occurs.

### 8.4 Dcache eligibility (Disabled mode)

- Create a filesystem with none of `NAMESPACE_IMMUTABLE/DCACHE_STRICT/DIR_CHANGE_TOKEN` set, and
  `ttl_ms` disabled.
- Enable dcache globally.
- Resolve the same path twice and assert the backend `lookup` counter increments both times.

### 8.5 Dir-change-token revalidation test

- Create a directory node that implements `dir_change_token()` and increments token on create/unlink.
- Ensure mount does *not* set `DCACHE_STRICT` (simulate “externally mutable / no invalidation”).
- Enable dcache and set mode to `RevalidateByDirToken` (via `DIR_CHANGE_TOKEN`).
- Resolve a path twice (second should hit).
- Mutate directory so token changes.
- Resolve again: must miss and refresh (must not return stale child).

---

## 9) Phased implementation checklist (junior-friendly)

### Phase A — Plumbing + data types

- [ ] Add `VfsCapabilities` bits to `vfs/core/src/capabilities.rs`:
  - [ ] `DCACHE_STRICT`
  - [ ] `NAMESPACE_IMMUTABLE`
  - [ ] `DIR_CHANGE_TOKEN`
- [ ] Add `DirChangeToken` + `dir_change_token()` methods to `FsNodeSync` and `FsNodeAsync`
  (defaulted to `Ok(None)`).
- [ ] Implement `vfs/core/src/dcache.rs` with:
  - [ ] `DcacheKey`
  - [ ] `DcacheSync` and `DcacheAsync` bounded LRU caches
  - [ ] `invalidate_one`, `invalidate_children_of`, `invalidate_mount`
- [ ] Add fast-cache dependencies to `vfs/core/Cargo.toml`:
  - [ ] `hashbrown`
  - [ ] `ahash`
  - [ ] (optional) `lru`

### Phase B — MountTable owns dcache

- [ ] Add `dcache: Option<Arc<Dcache>>` field to `MountTable`.
- [ ] Add `MountTable::new_with_dcache(...)` (or config equivalent).
- [ ] Add `MountTable::dcache()` getter.
- [ ] Add mount epochs:
  - [ ] store `mount_epoch` in `MountEntry` (or parallel mount metadata)
  - [ ] bump epoch each time a mount is installed into a slot
  - [ ] include `mount_epoch` in `DcacheKey`
- [ ] Implement mount lifecycle cache cleanup (space reclamation) in `vfs/core/src/mount.rs`:
  - [ ] clear on unmount removal
  - [ ] clear on detached reclaim

### Phase C — Read-path integration (PathWalker)

- [ ] Update `PathWalker` in `vfs/core/src/path_walker.rs` to consult dcache on `lookup` based on mode:
  - [ ] validate + policy checks unchanged
  - [ ] `Immutable` / `StrictInvalidate`: accept hits directly
  - [ ] `RevalidateByDirToken`: compare `dir_change_token()` and refresh on mismatch
  - [ ] `TTL`: expire entries
- [ ] Mirror in `PathWalkerAsync`.

### Phase D — Write-path invalidation (Vfs)

- [ ] Add invalidation calls after successful mutations in `vfs/core/src/vfs.rs`:
  - [ ] openat/openat_async (CREATE path)
  - [ ] mkdirat/mkdirat_async
  - [ ] unlinkat/unlinkat_async
  - [ ] renameat/renameat_async
  - [ ] symlinkat/symlinkat_async

### Phase E — Tests

- [ ] Cache hit test (lookup count)
- [ ] Mutation invalidation tests
- [ ] MountId reuse test (mount_epoch correctness)
- [ ] Disabled-mode gating test

---

## Appendix A — Follow-ups (optional improvements after v1)

### A.1 Negative caching

After positive caching is stable, consider negative caching:

- Store “not found” results with a short TTL, and invalidate on create under the same parent+name.
- This helps workloads that repeatedly probe missing paths (common in language runtimes).

### A.2 Watch-driven invalidation (Linux-like for externally mutable FS)

For backends that support reliable watch events (`FsProviderCapabilities::WATCH`), a future step is:

- subscribe to directory watches
- invalidate relevant dcache entries on events

This can make caching correct and low-latency even for externally mutable backends (hostfs),
approaching Linux’s coherence in practice.

