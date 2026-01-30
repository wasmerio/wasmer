//! Canonical path normalization and resolution.
//!
//! Phase 2.1 will implement mount-aware traversal, symlink resolution, trailing-slash semantics,
//! and the `PathWalker` contract described in `fs-refactor.md`.
//!
//! Path *types* live in `path_types.rs` to keep this module reserved for semantic walking logic.
