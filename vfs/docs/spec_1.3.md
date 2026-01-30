
Below is a **full, implementable specification** for **Step 1.3: Define filesystem provider abstraction** (`vfs/core/src/provider.rs`). It is written so a **junior engineer can implement it end-to-end**, while still fitting the constraints and downstream needs of the overall plan (mount table, path walker, node/handle, sync+async adapters, Wasix integration).

---

# Step 1.3 Specification — Filesystem Provider Abstraction (`vfs/core/src/provider.rs`)

## 0) Goal and non-goals

### Goal

Define the **provider layer contracts** that let the VFS mount *heterogeneous filesystem backends* (memfs, hostfs, overlay, object store) behind **one dynamic, capability-aware interface**:

* A **Linux-like separation**:

  * `FsProvider` = filesystem *type/driver* (like `file_system_type`)
  * `Fs` = filesystem *instance* (like superblock)
  * `Mount` handled later (Phase 3), but provider API must be mount-friendly
* Works with a **dynamic registry** (`dyn` trait objects)
* Explicit **capability model** that is truthful and used by VFS to gate semantics
* Supports **both async and sync usage paths** via adapters (runtime glue lives outside core)

### Non-goals (for 1.3)

* Implement mount table (`mount.rs`) or path traversal (`path.rs`)
* Implement full node/handle APIs (these land in Phase 2 and Phase 4)
* Implement concrete providers (memfs/hostfs/overlay)

This step **does** define the types and trait boundaries those later steps will plug into.

---

## 1) File structure and module layout

### Deliverable

Create `vfs/core/src/provider.rs` as a *small* module root that re-exports submodules in a directory:

```
vfs/core/src/provider/
  mod.rs              (or provider.rs as module root)
  capabilities.rs
  config.rs
  registry.rs
  provider.rs         (traits: FsProvider, Fs)
  adapters.rs         (AsyncAdapter, SyncAdapter scaffolding)
```

If the project insists on a single file at `provider.rs`, you may still structure it using internal `mod capabilities;` etc and place the real code in `provider/*.rs`. **No huge files**.

`vfs/core/src/lib.rs` should `pub mod provider;` and re-export the key types:

* `FsProvider`, `Fs`, `FsProviderRegistry`, `FsProviderCapabilities`, `FsCapabilities`, `MountSpec`, etc.

---

## 2) Core type designs

### 2.1 Provider and filesystem identity types

These types must be **cheap to copy**, easy to log, and stable.

```rust
/// Name used in registry lookup: "mem", "host", "overlay", ...
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProviderName(pub std::borrow::Cow<'static, str>);

/// Optional identifier for an Fs instance (used in debug/logging, watch routing, etc.).
/// Not required to be globally unique; unique within a registry/context is enough.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FsInstanceId(pub u64);
```

Rules:

* `ProviderName` is effectively the registry key.
* `FsInstanceId` is assigned by the registry at mount time (monotonic counter) unless the provider supplies one.

### 2.2 Capability flags (two levels)

We need **two** capability surfaces:

1. **Provider capabilities**: what the provider *can ever support* (e.g. hostfs supports hardlinks on Unix; memfs supports everything; object stores don’t).
2. **Fs instance capabilities**: what *this instance* supports given config (e.g. hostfs mounted on Windows may have different semantics; overlay mount may disable features; a hostfs mount may be read-only).

#### `FsProviderCapabilities` (static-ish)

Bitflags:

```rust
bitflags::bitflags! {
  #[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
  pub struct FsProviderCapabilities: u64 {
    const SYMLINK            = 1 << 0;
    const HARDLINK           = 1 << 1;
    const ATOMIC_RENAME      = 1 << 2;   // rename is atomic (POSIX expectation)
    const FILE_LOCKS         = 1 << 3;
    const XATTR              = 1 << 4;
    const SPARSE             = 1 << 5;
    const O_TMPFILE          = 1 << 6;
    const WATCH              = 1 << 7;

    const CASE_SENSITIVE     = 1 << 8;
    const CASE_PRESERVING    = 1 << 9;

    const UNIX_PERMISSIONS   = 1 << 10;  // uid/gid/mode are meaningful
    const UTIMENS            = 1 << 11;  // atime/mtime/ctime updates

    const STABLE_INODES      = 1 << 12;  // inode identity stable for mount lifetime
    const SEEK               = 1 << 13;  // seekable file handles
  }
}
```

#### `FsCapabilities` (per instance)

Same bitflags type or separate type:

* Use a separate `FsCapabilities` to allow the provider to “narrow” support per instance.
* `FsCapabilities` may equal `FsProviderCapabilities` internally, but keep the distinction in naming.

```rust
pub type FsCapabilities = FsProviderCapabilities;
```

Rule:

* Provider has `provider_capabilities()`
* Fs instance has `capabilities()`

### 2.3 Mount / instance flags (VFS-facing)

Even though mount table is Phase 3, providers need to know things like read-only.

```rust
bitflags::bitflags! {
  #[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
  pub struct MountFlags: u32 {
    const READ_ONLY  = 1 << 0;
    const NO_EXEC    = 1 << 1;
    const NO_SUID    = 1 << 2;
    const NO_DEV     = 1 << 3;
    // Add more later when mount.rs lands
  }
}
```

Mount flags are **VFS policy**, not provider truth. Provider may refine Fs capabilities accordingly (e.g. READ_ONLY clears write features).

---

## 3) Type-erased provider config (no serde required)

We need runtime registration and `mount_with_provider(name, config, ...)`, so config must be **type-erased**.

### 3.1 `ProviderConfig` trait

```rust
pub trait ProviderConfig: Send + Sync + std::fmt::Debug + 'static {
  fn as_any(&self) -> &dyn std::any::Any;
}

impl<T> ProviderConfig for T
where
  T: Send + Sync + std::fmt::Debug + 'static
{
  fn as_any(&self) -> &dyn std::any::Any { self }
}
```

### 3.2 Convenience alias

```rust
pub type ProviderConfigBox = Box<dyn ProviderConfig>;
```

### 3.3 Downcast helper

Provide a helper to make provider implementations easy and consistent:

```rust
pub fn config_downcast_ref<T: 'static>(cfg: &dyn ProviderConfig) -> Option<&T> {
  cfg.as_any().downcast_ref::<T>()
}
```

Rules:

* Providers must return `VfsError::InvalidInput` if config type mismatches (see error model in core).
* Config is only used at mount time; performance isn’t critical.

---

## 4) Provider trait design (dynamic, async-first, object-safe)

### 4.1 Why async-first here

* Registry must store `Arc<dyn FsProvider>` and mount via dyn dispatch.
* We will use `async-trait` (as required by the plan) so methods remain object-safe.
* Sync support is provided via adapters (see §6), with runtime hooks implemented in `vfs-rt`.

### 4.2 `FsProvider` trait (filesystem type/driver)

```rust
#[async_trait::async_trait]
pub trait FsProvider: Send + Sync {
  /// Registry-visible name, e.g. "mem", "host", "overlay".
  fn name(&self) -> &str;

  /// Provider-level capabilities (upper bound).
  fn provider_capabilities(&self) -> FsProviderCapabilities;

  /// Create/mount a new filesystem instance.
  ///
  /// - `cfg` is provider-specific (type-erased).
  /// - `flags` are VFS mount flags (read-only, etc.).
  /// - Returns an `Fs` instance usable by the mount table.
  async fn mount(
    &self,
    cfg: &dyn ProviderConfig,
    flags: MountFlags,
  ) -> crate::VfsResult<std::sync::Arc<dyn Fs>>;

  /// Optional fast validation hook (sync) to fail early before async mount work.
  ///
  /// Default: ok.
  fn validate_config(&self, _cfg: &dyn ProviderConfig) -> crate::VfsResult<()> {
    Ok(())
  }
}
```

Notes:

* `mount()` returns `Arc<dyn Fs>` (superblock-like).
* `validate_config()` allows fast error messages without spinning up runtime tasks; registry may call it before mount.

### 4.3 `Fs` trait (filesystem instance / superblock)

This is intentionally small for Step 1.3, but must include what later phases require.

```rust
#[async_trait::async_trait]
pub trait Fs: Send + Sync {
  /// Instance identifier (assigned by registry unless provider overrides).
  fn instance_id(&self) -> FsInstanceId;

  /// Per-instance capabilities (may be narrower than provider caps).
  fn capabilities(&self) -> FsCapabilities;

  /// Returns a backend root handle for traversal.
  ///
  /// NOTE: the concrete node interface is defined in Phase 2 / Phase 4.
  /// For now, return an opaque handle token (see §5).
  fn root_token(&self) -> crate::node::NodeToken;

  /// Optional watcher support hook:
  /// - If WATCH not present, VFS may emulate via polling later.
  fn watcher_token(&self) -> Option<crate::watch::WatcherToken> {
    None
  }
}
```

This requires **two opaque “token” types** to avoid depending on unfinished node/handle traits while keeping Step 1.3 complete.

---

## 5) Opaque tokens to decouple Step 1.3 from Phase 2/4

To make Step 1.3 implementable *now* without waiting for node/handle traits, we introduce **token handles** that will later be replaced or backed by real trait objects.

### 5.1 `NodeToken`

In `vfs/core/src/node.rs` later you will define real node traits. For Step 1.3, define a minimal token type in a small module (or in `provider.rs` behind a `pub mod node` shim) to keep compilation.

**In Step 1.3 scope**, define:

```rust
pub mod node {
  use std::sync::Arc;

  /// Temporary opaque token that will later wrap `Arc<dyn FsNode>`.
  /// This keeps provider.rs independent from the final node trait shape.
  #[derive(Clone)]
  pub struct NodeToken(pub Arc<dyn std::any::Any + Send + Sync>);
}
```

Rules:

* Providers store whatever they want inside `NodeToken` (commonly an `Arc<BackendNode>`).
* In Phase 2.3, you will replace `NodeToken` with a strongly typed `Arc<dyn FsNode>` and delete this token indirection.
* Until then, `NodeToken` is only passed around, not interpreted.

### 5.2 `WatcherToken` (optional)

Same idea:

```rust
pub mod watch {
  use std::sync::Arc;

  #[derive(Clone)]
  pub struct WatcherToken(pub Arc<dyn std::any::Any + Send + Sync>);
}
```

If you don’t want `watch` tokens yet, you can omit `watcher_token()` and add it later; but the plan explicitly references WATCH capability and emulation, so it’s better to include.

---

## 6) Provider registry (in this step)

Even though Phase 3.2 also mentions the registry, Step 1.3 requires one (mounting is a provider concern). Implement it here; Phase 3 can extend it with mount table wiring.

### 6.1 `FsProviderRegistry` type

Design constraints:

* **Per-context** registry, not global
* Must support:

  * `register_provider(name, provider)`
  * `get_provider(name)`
  * `mount_with_provider(name, cfg, flags)` → returns `Arc<dyn Fs>`
* Thread-safe
* Mount is rare; read access may be frequent but not hot-path like path-walking. `RwLock<HashMap<..>>` is sufficient.

```rust
pub struct FsProviderRegistry {
  inner: std::sync::RwLock<std::collections::HashMap<String, std::sync::Arc<dyn FsProvider>>>,
  next_fs_id: std::sync::atomic::AtomicU64,
}
```

### 6.2 Registry API

```rust
impl FsProviderRegistry {
  pub fn new() -> Self;

  /// Registers provider by `provider.name()`.
  /// Returns error if name already exists.
  pub fn register(&self, provider: std::sync::Arc<dyn FsProvider>) -> crate::VfsResult<()>;

  pub fn get(&self, name: &str) -> Option<std::sync::Arc<dyn FsProvider>>;

  /// Convenience: validate + mount + assign FsInstanceId if provider didn't.
  pub async fn mount_with_provider(
    &self,
    name: &str,
    cfg: &dyn ProviderConfig,
    flags: MountFlags,
  ) -> crate::VfsResult<std::sync::Arc<dyn Fs>>;
}
```

### 6.3 Assigning `FsInstanceId`

Because `Fs::instance_id()` exists, we need a way to set it.

**Rule:** The registry wraps the returned `Arc<dyn Fs>` with a small delegating wrapper that stores the `FsInstanceId` if the provider didn’t set one.

Implement:

```rust
struct FsInstanceWrapper {
  id: FsInstanceId,
  inner: std::sync::Arc<dyn Fs>,
}

#[async_trait::async_trait]
impl Fs for FsInstanceWrapper {
  fn instance_id(&self) -> FsInstanceId { self.id }
  fn capabilities(&self) -> FsCapabilities { self.inner.capabilities() }
  fn root_token(&self) -> crate::node::NodeToken { self.inner.root_token() }
  fn watcher_token(&self) -> Option<crate::watch::WatcherToken> { self.inner.watcher_token() }
}
```

**Important:** If the provider wants to manage IDs itself, allow a convention:

* Provider returns an Fs whose `instance_id()` is non-zero and registry respects it.
* Otherwise registry assigns a monotonically increasing `FsInstanceId`.

Because `FsInstanceId` is `u64`, we can treat `0` as “unset”.

---

## 7) Sync/Async adapters (scaffolding in this step)

The plan requires:

* Providers expose both sync + async call paths
* Avoid duplicate logic; implement one canonical trait + adapters
* Runtime coupling must live in `vfs/rt`

### 7.1 Runtime hook traits (declared in core, implemented in vfs-rt)

In core, define **minimal runtime hooks** as traits only—no tokio imports.

```rust
pub trait VfsRuntime: Send + Sync {
  fn spawn_blocking<F, R>(&self, f: F) -> std::pin::Pin<Box<dyn std::future::Future<Output = R> + Send>>
  where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static;

  fn block_on<F: std::future::Future>(&self, fut: F) -> F::Output;
}
```

* `vfs-rt` will provide implementations for tokio, async-std, or a custom runtime.
* Core does not depend on any runtime crate.

### 7.2 Adapter types (defined now, implemented fully when sync traits exist)

Phase 4 defines `FsProviderSync` and `FsProviderAsync`, etc. Step 1.3 should include the adapter **type designs and behavior**, even if some methods are `todo!()` until Phase 4 lands.

#### `AsyncAdapter<T>` (sync → async)

```rust
pub struct AsyncAdapter<T> {
  inner: T,
  rt: std::sync::Arc<dyn VfsRuntime>,
}
```

Behavior:

* `FsProvider` is implemented for `AsyncAdapter<T>` where `T: FsProviderSync`.
* `mount()` calls `rt.spawn_blocking(move || inner.mount_sync(cfg, flags))` and awaits it.
* Same pattern will apply for `Fs`, nodes, handles once those traits exist.

#### `SyncAdapter<T>` (async → sync)

```rust
pub struct SyncAdapter<T> {
  inner: T,
  rt: std::sync::Arc<dyn VfsRuntime>,
}
```

Behavior:

* Implements `FsProviderSync` for `SyncAdapter<T>` where `T: FsProvider` (async).
* `mount_sync()` calls `rt.block_on(inner.mount(cfg, flags))`.

**Rule:** Adapters must preserve error identity (`VfsError`) and must not “wrap” errors in opaque runtime errors.

---

## 8) Capability gating rules (how VFS will use them)

Even though enforcement happens later, Step 1.3 must define **what the flags mean** so providers set them truthfully.

### 8.1 Required capability semantics

* `SYMLINK`: backend can create/resolve symlinks meaningfully (readlink + follow)
* `HARDLINK`: backend supports hard links to the same inode identity
* `ATOMIC_RENAME`: `rename(a,b)` is atomic within the same fs instance
* `STABLE_INODES`: inode identity stable for mount lifetime (needed for mount table inode-driven transitions and for caching)
* `WATCH`: backend can emit watch events without polling
* `CASE_SENSITIVE` / `CASE_PRESERVING`: describes lookup rules and name presentation
* `UNIX_PERMISSIONS`: uid/gid/mode are meaningful and enforced/returned
* `UTIMENS`: timestamps meaningful and can be set (subject to perms)
* `SEEK`: handles are seekable (some object store streams may not be)

### 8.2 “Truthfulness contract”

A provider must clear a capability if it cannot guarantee the semantics **reliably**.

* Example: object store **must** clear `ATOMIC_RENAME`, `HARDLINK`, `STABLE_INODES` (unless it implements a stable mapping table), etc.
* VFS will map unsupported operations to `VfsError::NotSupported` (later mapped to WASI errno).

---

## 9) Error expectations for this step

Step 1.3 must not invent its own error model; it uses `crate::{VfsError,VfsResult}` from Step 1.2.

Registry errors must be:

* Duplicate provider name: `VfsError::AlreadyExists`
* Provider missing: `VfsError::NotFound`
* Config mismatch: `VfsError::InvalidInput`
* Provider mount failure: pass-through (do not rewrap)

---

## 10) Implementation checklist (junior-engineer oriented)

### 10.1 Add dependencies (core crate)

In `vfs/core/Cargo.toml`:

* `async-trait`
* `bitflags`

No runtime deps.

### 10.2 Implement modules

1. `capabilities.rs`

   * define `FsProviderCapabilities`, `FsCapabilities`, `MountFlags`
2. `config.rs`

   * define `ProviderConfig`, `ProviderConfigBox`, `config_downcast_ref`
3. `provider.rs` (traits)

   * define `FsProvider`, `Fs`, `ProviderName`, `FsInstanceId`
4. `registry.rs`

   * implement `FsProviderRegistry`
   * implement `FsInstanceWrapper` pattern
5. `adapters.rs`

   * define `VfsRuntime` trait
   * define `AsyncAdapter`, `SyncAdapter` structs + docs
   * actual trait impls can be completed in Phase 4, but the types and runtime contract must be correct now

### 10.3 Unit tests (must be included in this step)

Create `vfs/core/src/provider/tests.rs` or `vfs/core/tests/provider_registry.rs`.

Tests:

1. **register + get**

   * register dummy provider `"dummy"`
   * `get("dummy")` returns Some
2. **duplicate register fails**

   * register `"dummy"` twice → `AlreadyExists`
3. **mount_with_provider calls mount**

   * dummy provider increments an `AtomicUsize` on mount
   * verify called exactly once
4. **config mismatch produces InvalidInput**

   * dummy provider expects `DummyCfg`
   * pass `OtherCfg` and ensure `InvalidInput`

Dummy provider should implement `FsProvider` and return a dummy `Fs` with a `root_token()`.

---

## 11) Acceptance criteria for Step 1.3

This is what “done” means for 1.3:

* `cargo check -p vfs-core` succeeds
* `FsProviderCapabilities`, `MountFlags`, `ProviderConfig`, and `FsProviderRegistry` are implemented and tested
* Registry is per-context (no global mutable state)
* Provider trait is **object-safe** and uses `async-trait`
* Adapter *types* and `VfsRuntime` hook trait are defined (even if full adapter impls are finished in Phase 4)
* No tokio dependency in `vfs/core`

---

## 12) Notes for downstream steps (so this step doesn’t paint us into a corner)

* **Phase 2.3/2.4** will replace `NodeToken` with real `Arc<dyn FsNode>` and propagate that through `Fs::root()`.

  * When that happens, delete the `Any`-based token approach.
* **Phase 3** will add `mount_with_provider(name, config, target_path, flags)` that attaches the returned `Fs` into the mount table. This step only returns `Fs`; it should not know about `MountId`.
* **Phase 4** will define the full sync/async node/handle traits and complete the adapter impls. The runtime hook trait defined here is the bridge point.

---

If you want, I can also provide a **concrete Rust skeleton** for `provider/` modules (compiling, with tests) that follows this spec exactly—but I’ll keep this response focused on the detailed specification as requested.
