
IMPORTANT NOTE: If details in related types have slightly diverged, prefer to keep the current status
and adapt the implementation accordingly, unless it hinders the delivery of this plan!!!

## Step 3.2 — Provider registry (explicit) — Detailed spec

### What step 3.2 is responsible for

Step 3.2 extracts and “locks in” a **first-class provider registry module** that the mount layer (3.1) uses to instantiate filesystem instances (`Arc<dyn Fs>`) from named providers (`Arc<dyn FsProvider>`), with:

* **Per-context ownership** (no global singleton by default).
* **Thread-safe registration + lookup** with low overhead on the hot path.
* **Clear error behavior** (duplicate providers, missing providers, invalid configs).
* **Capability introspection** exposed in a stable way (for gating and better diagnostics).
* **A mount instantiation API** that cleanly composes with mount-table attach logic (3.1) and path resolution (3.3), without re-implementing mount semantics in the registry.

This step does **not** implement mount table transitions or path walking. It provides the clean registry surface that those steps depend on.

---

## Relationship to other phases/steps

### How 3.2 plugs into 3.1 (mount table)

* **3.1 owns mount semantics**: validating mountpoints, tracking mount IDs, parent/child mounts, lazy unmount, busy semantics, mount flags enforcement, inode-driven mount transitions.
* **3.2 only creates `Arc<dyn Fs>`** from `(provider_name, config, flags, target_path)` and returns it (or an error).
* **3.1 calls 3.2** during “mount” to get an `Fs` instance, then **attaches** it into the mount table snapshot.

**Invariant:** the registry never mutates the mount table; it returns an `Fs` instance that mount.rs attaches.

### How 3.2 relates to 2.x core semantics

* 2.1/3.3 `PathWalker` must never depend on provider names or configs; it only deals in `MountId`, `VfsInodeId`, and mount transitions. Registry is not on the hot traversal path.
* 2.2 (inode IDs) and 2.3 (Fs/FsNode) define the objects returned by providers. Registry should treat them as opaque trait objects.
* 2.5 (permissions/policy): registry does not enforce permission checks; those are VFS-level semantics (and mount policies) applied during operations and/or mount attach (3.1), not during provider lookup.

### How 3.2 sets up Phase 4 (sync/async traits)

* Your current `FsProvider` trait is sync (`mount` returns `VfsResult<Arc<dyn Fs>>`).
* Phase 4 introduces `FsProviderSync` / `FsProviderAsync` and adapters. Step 3.2 must:

  * Keep the registry interface **object-safe** and **provider-agnostic**.
  * Avoid baking in tokio or any runtime.
  * Keep the mount instantiation API shaped so it can later accept “async mount” via adapters without changing mount table semantics.

---

## Deliverables

### New module/file

Create: `vfs/core/src/provider_registry.rs`

* Move the registry type out of `provider.rs` into this module.
* `provider.rs` should retain:

  * `FsProvider`, `MountRequest`, `ProviderConfig` + helpers
  * capability flags (`FsProviderCapabilities`, `MountFlags`)
  * runtime traits/adapters can remain here for now (or later move to `rt` crate), but do not entangle the registry with runtime glue.

### Public API surface (re-exports)

In `vfs/core/src/lib.rs` (or `mod.rs`), re-export the registry so downstream crates don’t care where it lives:

* `pub use crate::provider_registry::FsProviderRegistry;`

Optionally keep a compatibility re-export in `provider.rs` temporarily:

* `pub use crate::provider_registry::FsProviderRegistry;`
* Mark old path as deprecated later once callers are updated.

---

## Core design principles

1. **Per-context registry**
   No global `lazy_static` registry. The owning “VFS context” (eventually Wasix env) stores an instance:

   * avoids cross-test interference
   * avoids surprising global mutable state
   * supports future multi-tenant / multiple namespaces

2. **Read-mostly performance**
   Provider lookups happen on mount operations (not every path component), so performance is less critical than mount table reads — but still:

   * lookups should be lock-light
   * registration is rare

3. **Provider identity is string name, normalized**
   Provider names must be canonicalized deterministically (see below).

4. **Registry owns *providers*, mount table owns *mounted instances***
   Registry returns `Arc<dyn Fs>`; mount table assigns a `MountId` and manages lifetime/visibility.

---

## Type and API specification

### Provider name normalization

Define a normalization function used on registration and lookup:

* Trim ASCII whitespace.
* Convert to ASCII lowercase.
* Reject empty names.
* Restrict to a safe character set to prevent weird logs/diagnostics:

  * Allowed: `[a-z0-9._-]`
  * Reject anything else with `InvalidInput`.

This prevents “Host” vs “host” or Unicode confusables.

**Implementation detail (junior-friendly):**

* Implement `fn normalize_provider_name(input: &str) -> VfsResult<String>` in `provider_registry.rs`.
* Use it in both `register_provider()` and `get()`/`mount_with_provider()`.

### Registry storage and concurrency

Define:

```rust
pub struct FsProviderRegistry {
    providers: RwLock<HashMap<String, Arc<dyn FsProvider>>>,
}
```

This matches your existing code and is sufficient for step 3.2.

**Locking rules:**

* `register_provider`: write-lock.
* `get`, `list_names`, `describe`: read-lock.
* Never hold a lock while calling into provider code (`validate_config`, `mount`) — clone the `Arc` first, drop lock, then call provider methods. This avoids deadlocks and long lock holds.

### Registry API

Implement these public methods (some exist already; spec clarifies behavior):

#### Constructors

* `pub fn new() -> Self`
* `impl Default`

#### Registration

* `pub fn register(&self, provider: Arc<dyn FsProvider>) -> VfsResult<()>`

  * Equivalent to `register_provider(provider.name(), provider)` after normalization.
  * Must validate `provider.name()` with normalization rules; error if invalid.

* `pub fn register_provider(&self, name: impl AsRef<str>, provider: Arc<dyn FsProvider>) -> VfsResult<()>`

  * Normalize `name`.
  * Error `AlreadyExists` if normalized name is present.
  * Store `Arc` as-is.

**No unregister** in v1 unless tests need it. Prefer constructing fresh registries in tests.

#### Lookup and listing

* `pub fn get(&self, name: &str) -> Option<Arc<dyn FsProvider>>`

  * Normalize; if normalization fails, return `None` (or choose to return `Result<Option<...>>`; see below).
  * Return cloned `Arc`.

**Recommendation (more explicit):** make it `VfsResult<Option<Arc<dyn FsProvider>>>` so invalid names become `InvalidInput` rather than “not found”. If you keep `Option`, document that invalid names are treated as missing.

* `pub fn list_names(&self) -> Vec<String>`

  * Return sorted normalized names.
  * If lock is poisoned, return empty vec (as your code does) **or** return `Internal`. For consistency with other core modules, prefer returning `VfsResult<Vec<String>>` and mapping poison to `Internal`.
  * Choose one approach and use it consistently.

#### Capability introspection (“explicit” requirement)

Add a structured way to query what’s available without mounting:

* `pub fn provider_capabilities(&self, name: &str) -> VfsResult<FsProviderCapabilities>`

  * Lookup provider; if missing → `NotFound`
  * Return `provider.provider_capabilities()` (or `capabilities()`)

* `pub fn describe_provider(&self, name: &str) -> VfsResult<ProviderInfo>`

  * `ProviderInfo` contains:

    ```rust
    pub struct ProviderInfo {
        pub name: String, // normalized
        pub capabilities: FsProviderCapabilities,
    }
    ```
  * (Optionally later) include a `debug_name` or version string if providers expose it.

* `pub fn list_providers(&self) -> VfsResult<Vec<ProviderInfo>>`

  * Read-lock, map each provider to info, sort by name.
  * Useful for diagnostics in Wasix and tests.

#### Mount instantiation API

Keep a registry-level helper that *only* instantiates an `Fs`:

* `pub fn create_fs(&self, provider_name: &str, req: MountRequest<'_>) -> VfsResult<Arc<dyn Fs>>`

And/or preserve the existing signature you already have:

* `pub fn mount_with_provider(&self, provider_name: &str, config: &dyn ProviderConfig, target_path: &VfsPath, flags: MountFlags) -> VfsResult<Arc<dyn Fs>>`

**But rename recommendation:** in Phase 3, the word “mount” becomes ambiguous because mount.rs is the real mount system. Prefer:

* `create_fs_with_provider(...)`
* or `instantiate_fs(...)`

**Required behavior:**

1. Normalize `provider_name`.
2. Read-lock registry, clone provider Arc, drop lock.
3. Call `provider.validate_config(config)` first.
4. Construct `MountRequest { target_path, flags, config }` and call `provider.mount(req)`.
5. Return the `Arc<dyn Fs>`.

**Important:** Do not attach to mount table here. That is 3.1.

#### Optional: validate mount flags vs provider capabilities (preflight)

This is *not strictly required* for 3.2, but it often helps.

Add a helper:

* `pub fn validate_mount_requirements(&self, provider_name: &str, flags: MountFlags, required: FsProviderCapabilities) -> VfsResult<()>`

Use cases:

* overlay provider might require `SYMLINK` or stable inode behavior from an upper layer.
* hostfs might require `CASE_SENSITIVE` assumptions in some environments (or not).

**Rule:** this is only a preflight diagnostic / early error. The real semantics and failure modes still happen at operation time based on `Fs::capabilities()` and mount flags.

---

## Error model (must be consistent across VFS)

Use `VfsErrorKind` consistently:

* `AlreadyExists`: registering a provider name twice.
* `NotFound`: mounting with unknown provider.
* `InvalidInput`: invalid provider name, config mismatch (provider chooses).
* `Internal`: lock poisoning, invariant violations.

**Lock poisoning policy (pick one):**

* Prefer returning `Internal("provider_registry.lock")` consistently (like your current code) instead of silently returning empty lists.

---

## Interaction with existing code in `provider.rs`

You already have:

* `FsProviderRegistry` implemented in `provider.rs`
* `ProviderConfig` + downcast helper
* `MountRequest`
* `FsProvider` trait

### Required refactor steps

1. **Create `provider_registry.rs`** and move `FsProviderRegistry` there.
2. Keep all current tests; update imports.
3. Add normalization + capability introspection APIs in the new module.
4. Update `provider.rs` to:

   * `pub mod provider_registry;` in lib root
   * re-export `FsProviderRegistry` if needed
5. Adjust any other modules (especially `mount.rs`) to import from the new module, not from `provider.rs`, to match the plan step naming.

---

## How mount.rs (3.1) should call the registry

Define the mount flow contract between 3.1 and 3.2:

1. mount.rs validates `target_path` at the VFS layer (absolute, normalized expectations, mountpoint existence, not inside a detached mount, etc.).
2. mount.rs calls:

   * `registry.create_fs_with_provider(provider_name, config, target_path, flags)`
3. mount.rs assigns:

   * new `MountId`
   * mount entry containing `Arc<dyn Fs>` plus mount metadata
4. mount.rs publishes updated mount table snapshot.

**Critical:** provider registry must not assume the mountpoint exists; it only receives `target_path` for provider context/logging and validation.

---

## Tests required for 3.2

You already have good unit tests. Extend them to cover the “explicit” requirements:

1. **Name normalization**

   * Register with `"HoSt"` and ensure lookup by `"host"` succeeds.
   * Reject invalid names (`""`, `"   "`, `"host!"`, `"☃"`).

2. **Capabilities introspection**

   * Dummy provider returns specific `FsProviderCapabilities`.
   * `provider_capabilities("dummy")` returns expected flags.
   * `list_providers()` returns sorted list.

3. **No locks held across provider calls**

   * Hard to test directly, but you can create a provider whose `mount()` tries to call back into registry listing; if registry still held lock, it would deadlock. The test should complete successfully.

4. **Error kinds**

   * Duplicate registration → `AlreadyExists`
   * Missing provider → `NotFound`
   * Config mismatch remains `InvalidInput` (provider-defined)

---

## Acceptance criteria for step 3.2

* `vfs-core` builds with the new module structure.
* All existing provider tests pass, updated to new module path.
* Added tests for normalization and capability introspection pass.
* mount.rs can depend on the registry module without cyclic dependencies.
* Registry never holds its internal lock while calling provider code.

---

## Suggested file layout after this step

* `vfs/core/src/provider.rs`

  * `FsProviderCapabilities`, `MountFlags`
  * `ProviderConfig` + downcast helpers
  * `MountRequest`
  * `FsProvider` trait
  * (runtime traits/adapters can remain here until Phase 4)

* `vfs/core/src/provider_registry.rs`

  * `FsProviderRegistry`
  * name normalization helper
  * `ProviderInfo` and introspection APIs
  * `create_fs_with_provider` / `mount_with_provider`

---

## Notes for future steps (don’t implement now, but don’t block them)

* **Async providers (Phase 4):** registry should remain usable with `dyn` providers once async traits exist. That means either:

  * keep registry typed over a “canonical provider trait” that is object-safe, and provide adapters, or
  * maintain two registries (sync/async) behind a single façade (usually not necessary).
* **Provider-scoped shared state:** Some providers may want internal caches; that lives inside the provider instance (the `Arc<dyn FsProvider>`), not the registry.
* **Mount namespaces:** if later required, registry remains per-context; namespaces belong to mount tables, not registries.

If you want, I can also draft the concrete Rust signatures for `provider_registry.rs` (with minimal diff from your existing file) so a junior dev can implement it mechanically.
