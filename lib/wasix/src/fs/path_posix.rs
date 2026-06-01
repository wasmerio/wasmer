//! Guest POSIX path helpers.
//!
//! WASIX implements a system layer, so guest paths must behave identically to
//! POSIX paths even when the runtime host is not POSIX. A different textual
//! form of the same path can still be observable to compat tests, so guest path
//! operations should preserve slash-separated string semantics instead of using
//! host-native `Path` component rules. Host-native `Path` remains the filesystem
//! trait boundary type; these types are for guest-visible path math on our side
//! of that boundary.

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

#[cfg(feature = "enable-serde")]
use serde_derive::{Deserialize, Serialize};
use wasmer_wasix_types::wasi::Errno;

#[derive(Clone, Copy)]
pub(crate) enum PosixPathComponent<'a> {
    RootDir,
    CurDir,
    ParentDir,
    Normal(&'a str),
}

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct PosixPath<'a> {
    #[cfg_attr(feature = "enable-serde", serde(borrow))]
    path: Cow<'a, str>,
}

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct PosixPathBuf {
    path: String,
}

impl<'a> PosixPath<'a> {
    pub(crate) fn new(path: &'a str) -> Self {
        Self {
            path: Cow::Borrowed(path),
        }
    }

    pub(crate) fn from_path(path: &'a Path) -> Self {
        Self {
            path: path.to_string_lossy(),
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        self.path.as_ref()
    }

    pub(crate) fn is_absolute(&self) -> bool {
        self.as_str().starts_with('/')
    }

    pub(crate) fn strip_root_prefix(&self) -> PosixPathBuf {
        PosixPathBuf::from(self.as_str().strip_prefix('/').unwrap_or(self.as_str()))
    }

    pub(crate) fn strip_prefix<'b>(&'b self, prefix: &PosixPath<'_>) -> Option<PosixPath<'b>> {
        let path = self.as_str();
        let prefix = prefix.as_str();

        if prefix == "/" {
            return path.strip_prefix('/').map(PosixPath::new);
        }

        if path == prefix {
            return Some(PosixPath::new(""));
        }

        let suffix = path.strip_prefix(prefix)?;
        suffix.strip_prefix('/').map(PosixPath::new)
    }

    pub(crate) fn parent(&self) -> PosixPathBuf {
        let path = self.as_str();
        let trimmed = path.trim_end_matches('/');
        let parent = trimmed
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or_default();
        PosixPathBuf::from(parent)
    }

    pub(crate) fn components(
        &self,
        include_root: bool,
        preserve_trailing_slash: bool,
    ) -> Vec<PosixPathComponent<'_>> {
        let path = self.as_str();
        let mut components = Vec::new();

        if include_root && path.starts_with('/') {
            components.push(PosixPathComponent::RootDir);
        }

        for component in path.split('/').filter(|component| !component.is_empty()) {
            components.push(match component {
                "." => PosixPathComponent::CurDir,
                ".." => PosixPathComponent::ParentDir,
                component => PosixPathComponent::Normal(component),
            });
        }

        if preserve_trailing_slash && path.ends_with('/') {
            components.push(PosixPathComponent::CurDir);
        }

        components
    }

    pub(crate) fn join(&self, relative: &PosixPath<'_>) -> PosixPathBuf {
        let base = self.as_str();
        let relative = relative.as_str();

        if relative.is_empty() || relative == "." {
            return PosixPathBuf::from(base);
        }

        if relative.starts_with('/') || base.is_empty() || base == "." {
            PosixPathBuf::from(relative)
        } else if base == "/" {
            PosixPathBuf::from(format!("/{relative}"))
        } else if base.ends_with('/') {
            PosixPathBuf::from(format!("{base}{relative}"))
        } else {
            PosixPathBuf::from(format!("{base}/{relative}"))
        }
    }

    pub(crate) fn parent_path_and_name(&self) -> Result<(PosixPathBuf, String), Errno> {
        let path = self.as_str();
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(Errno::Inval);
        }

        let (parent, name) = match trimmed.rsplit_once('/') {
            Some(("", name)) if path.starts_with('/') => ("/", name),
            Some((parent, name)) => (parent, name),
            None => ("", trimmed),
        };

        if name.is_empty() {
            return Err(Errno::Inval);
        }

        Ok((PosixPathBuf::from(parent), name.to_string()))
    }

    pub(crate) fn normalize_virtual_symlink_key(&self) -> PosixPathBuf {
        let mut normalized = Vec::new();

        for component in self.components(false, false) {
            match component {
                PosixPathComponent::RootDir | PosixPathComponent::CurDir => {}
                PosixPathComponent::ParentDir => {
                    normalized.pop();
                }
                PosixPathComponent::Normal(component) => normalized.push(component.to_owned()),
            }
        }

        PosixPathBuf::from_components(self.is_absolute(), &normalized, "/")
    }
}

impl PosixPathBuf {
    pub(crate) fn from_components(
        is_absolute: bool,
        components: &[String],
        empty_path: &str,
    ) -> Self {
        if components.is_empty() {
            PosixPathBuf::from(empty_path)
        } else if is_absolute {
            PosixPathBuf::from(format!("/{}", components.join("/")))
        } else {
            PosixPathBuf::from(components.join("/"))
        }
    }

    pub(crate) fn as_posix_path(&self) -> PosixPath<'_> {
        PosixPath::new(&self.path)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.path
    }

    pub(crate) fn into_path_buf(self) -> PathBuf {
        PathBuf::from(self.path)
    }

    pub(crate) fn resolve_relative(
        symlink_parent: &PosixPath<'_>,
        relative_path: &PosixPath<'_>,
        preserve_after_first_normal: bool,
    ) -> Result<Self, Errno> {
        let mut resolved = Vec::new();
        symlink_parent.push_normalized_relative(&mut resolved)?;

        if !preserve_after_first_normal {
            relative_path.push_normalized_relative(&mut resolved)?;
            return Ok(PosixPathBuf::from_components(false, &resolved, "."));
        }

        let mut validation = resolved.clone();
        relative_path.push_normalized_relative(&mut validation)?;

        let mut remaining = Vec::new();
        let mut preserve_remaining = false;

        relative_path.visit_relative_components(|component| {
            if preserve_remaining {
                match component {
                    PosixPathComponent::RootDir => {}
                    PosixPathComponent::CurDir => remaining.push(".".to_owned()),
                    PosixPathComponent::ParentDir => remaining.push("..".to_owned()),
                    PosixPathComponent::Normal(component) => remaining.push(component.to_owned()),
                }
                return Ok(());
            }

            match component {
                PosixPathComponent::RootDir | PosixPathComponent::CurDir => {}
                PosixPathComponent::ParentDir => {
                    resolved.pop().ok_or(Errno::Perm)?;
                }
                PosixPathComponent::Normal(component) => {
                    remaining.push(component.to_owned());
                    preserve_remaining = true;
                }
            }
            Ok(())
        })?;

        if !remaining.is_empty() {
            resolved.extend(remaining);
        }

        Ok(PosixPathBuf::from_components(false, &resolved, "."))
    }
}

impl<'a> PosixPath<'a> {
    fn visit_relative_components<'b, F>(&'b self, mut visit: F) -> Result<(), Errno>
    where
        F: FnMut(PosixPathComponent<'b>) -> Result<(), Errno>,
    {
        if self.is_absolute() {
            return Err(Errno::Perm);
        }

        for component in self.components(false, false) {
            visit(component)?;
        }

        Ok(())
    }

    fn push_normalized_relative(&self, resolved: &mut Vec<String>) -> Result<(), Errno> {
        self.visit_relative_components(|component| {
            match component {
                PosixPathComponent::RootDir | PosixPathComponent::CurDir => {}
                PosixPathComponent::Normal(component) => resolved.push(component.to_owned()),
                PosixPathComponent::ParentDir => {
                    resolved.pop().ok_or(Errno::Perm)?;
                }
            }
            Ok(())
        })
    }
}

impl From<&str> for PosixPathBuf {
    fn from(path: &str) -> Self {
        Self {
            path: path.to_owned(),
        }
    }
}

impl From<String> for PosixPathBuf {
    fn from(path: String) -> Self {
        Self { path }
    }
}
