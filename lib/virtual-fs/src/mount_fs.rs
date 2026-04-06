//! A mount-topology filesystem that routes operations by path,
//! its not as simple as TmpFs. not currently used but was used by
//! the previoulsy implementation of Deploy - now using TmpFs

use dashmap::{DashMap, mapref::entry::Entry};

use crate::*;

use std::{
    collections::BTreeSet,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};

type DynFileSystem = Arc<dyn FileSystem + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExactMountConflictMode {
    Fail,
    KeepExisting,
    ReplaceExisting,
}

#[derive(Debug, Clone)]
struct MountedFileSystem {
    fs: DynFileSystem,
}

#[derive(Debug, Default)]
struct MountNode {
    mount: DashMap<(), MountedFileSystem>,
    children: DashMap<OsString, Arc<MountNode>>,
}

#[derive(Debug, Clone)]
struct ExactNode {
    path: PathBuf,
    fs: Option<DynFileSystem>,
    child_names: BTreeSet<OsString>,
}

impl ExactNode {
    fn has_children(&self) -> bool {
        !self.child_names.is_empty()
    }
}

#[derive(Debug, Clone)]
struct ResolvedMount {
    mount_path: PathBuf,
    delegated_path: PathBuf,
    fs: DynFileSystem,
}

#[derive(Debug, Clone)]
pub struct MountPoint {
    pub path: PathBuf,
    pub name: String,
    pub fs: Option<DynFileSystem>,
    pub children: Option<Arc<MountFileSystem>>,
}

#[derive(Debug, Clone)]
pub struct MountEntry {
    pub path: PathBuf,
    pub fs: DynFileSystem,
}

impl MountPoint {
    pub fn fs(&self) -> Option<&(dyn FileSystem + Send + Sync)> {
        self.fs.as_deref()
    }

    pub fn mount_point_ref(&self) -> MountPointRef<'_> {
        MountPointRef {
            path: self.path.clone(),
            name: self.name.clone(),
            fs: self.fs.as_deref(),
        }
    }
}

/// Allows different filesystems of different types
/// to be mounted at various mount points
#[derive(Debug, Default)]
pub struct MountFileSystem {
    root: Arc<MountNode>,
}

impl MountFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mount(
        &self,
        path: &Path,
        fs: Box<dyn FileSystem + Send + Sync>,
    ) -> Result<()> {
        let path = self.prepare_path(path)?;
        let node = self.mount_node(&Self::path_components(&path));

        match node.mount.entry(()) {
            Entry::Occupied(_) => Err(FsError::AlreadyExists),
            Entry::Vacant(slot) => {
                slot.insert(MountedFileSystem { fs: Arc::from(fs) });
                Ok(())
            }
        }
    }

    pub fn filesystem_at(&self, path: &Path) -> Option<Arc<dyn FileSystem + Send + Sync>> {
        self.exact_node(path).and_then(|node| node.fs)
    }
    pub fn clear(&mut self) {
        self.root = Arc::new(MountNode::default());
    }

    fn prepare_path(&self, path: &Path) -> Result<PathBuf> {
        let mut normalized = PathBuf::new();

        for component in path.components() {
            match component {
                std::path::Component::RootDir | std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if !normalized.pop() {
                        return Err(FsError::InvalidInput);
                    }
                }
                std::path::Component::Normal(part) => normalized.push(part),
                std::path::Component::Prefix(_) => return Err(FsError::InvalidInput),
            }
        }

        Ok(normalized)
    }

    fn path_components(path: &Path) -> Vec<OsString> {
        path.components()
            .map(|component| component.as_os_str().to_os_string())
            .collect()
    }

    fn absolute_path(components: &[OsString]) -> PathBuf {
        let mut path = PathBuf::from("/");
        for component in components {
            path.push(component);
        }
        path
    }

    fn directory_metadata() -> Metadata {
        Metadata {
            ft: FileType::new_dir(),
            accessed: 0,
            created: 0,
            modified: 0,
            len: 0,
        }
    }

    fn should_fallback_to_synthetic_dir(error: &FsError) -> bool {
        matches!(
            error,
            FsError::Unsupported | FsError::NotAFile | FsError::BaseNotDirectory
        )
    }

    fn synthetic_entry(name: OsString, base: &Path) -> DirEntry {
        DirEntry {
            path: base.join(PathBuf::from(name)),
            metadata: Ok(Self::directory_metadata()),
        }
    }

    fn mounted(node: &Arc<MountNode>) -> Option<MountedFileSystem> {
        node.mount.get(&()).map(|mount| mount.clone())
    }

    fn collect_mount_entries(
        node: &Arc<MountNode>,
        path: &Path,
        entries: &mut Vec<MountEntry>,
    ) {
        if let Some(mount) = Self::mounted(node) {
            entries.push(MountEntry {
                path: path.to_path_buf(),
                fs: mount.fs,
            });
        }

        let mut child_names = node
            .children
            .iter()
            .map(|child| child.key().clone())
            .collect::<Vec<_>>();
        child_names.sort();

        for child_name in child_names {
            let Some(child) = node
                .children
                .get(&child_name)
                .map(|entry| Arc::clone(entry.value()))
            else {
                continue;
            };
            let child_path = path.join(&child_name);
            Self::collect_mount_entries(&child, &child_path, entries);
        }
    }

    fn find_node(&self, path: &Path) -> Option<Arc<MountNode>> {
        let path = self.prepare_path(path).ok()?;
        let mut node = Arc::clone(&self.root);

        for component in Self::path_components(&path) {
            let child = node
                .children
                .get(&component)
                .map(|entry| Arc::clone(entry.value()))?;
            node = child;
        }

        Some(node)
    }

    fn exact_node(&self, path: &Path) -> Option<ExactNode> {
        let path = self.prepare_path(path).ok()?;
        let visible_path = Path::new("/").join(&path);
        let node = self.find_node(&path)?;
        let mounted = Self::mounted(&node);

        Some(ExactNode {
            path: visible_path.clone(),
            fs: mounted.map(|mount| mount.fs),
            child_names: node
                .children
                .iter()
                .map(|child| child.key().clone())
                .collect(),
        })
    }

    fn resolve_mount(&self, path: PathBuf) -> Option<ResolvedMount> {
        let path = self.prepare_path(&path).ok()?;
        let components = Self::path_components(&path);
        let mut node = Arc::clone(&self.root);
        let mut best = Self::mounted(&node).map(|mount| ResolvedMount {
            mount_path: PathBuf::from("/"),
            delegated_path: Self::absolute_path(&components),
            fs: mount.fs,
        });

        for (index, component) in components.iter().enumerate() {
            let Some(child) = node
                .children
                .get(component)
                .map(|entry| Arc::clone(entry.value()))
            else {
                break;
            };
            node = child;

            if let Some(mount) = Self::mounted(&node) {
                best = Some(ResolvedMount {
                    mount_path: Self::absolute_path(&components[..=index]),
                    delegated_path: Self::absolute_path(&components[index + 1..]),
                    fs: mount.fs,
                });
            }
        }

        best
    }

    fn rebase_entries(entries: &mut ReadDir, source_prefix: &Path, target_prefix: &Path) {
        for entry in &mut entries.data {
            let suffix = entry.path.strip_prefix(source_prefix).unwrap_or_else(|_| {
                entry
                    .path
                    .strip_prefix(Path::new("/"))
                    .unwrap_or(&entry.path)
            });
            entry.path = target_prefix.join(suffix);
        }
    }

    fn read_dir_from_exact_node(&self, node: &ExactNode) -> Result<ReadDir> {
        let mut entries = Vec::new();

        if let Some(fs) = &node.fs {
            match fs.read_dir(Path::new("/")) {
                Ok(mut base_entries) => {
                    Self::rebase_entries(&mut base_entries, Path::new("/"), &node.path);
                    entries.extend(base_entries.data.into_iter().filter(|entry| {
                        entry
                            .path
                            .file_name()
                            .map(|name| !node.child_names.contains(name))
                            .unwrap_or(true)
                    }));
                }
                Err(error)
                    if node.has_children()
                        && Self::should_fallback_to_synthetic_dir(&error) => {}
                Err(error) => return Err(error),
            }
        }

        entries.extend(
            node.child_names
                .iter()
                .cloned()
                .map(|name| Self::synthetic_entry(name, &node.path)),
        );

        Ok(ReadDir::new(entries))
    }

    fn mount_node(&self, components: &[OsString]) -> Arc<MountNode> {
        let mut node = Arc::clone(&self.root);

        for component in components {
            let next = match node.children.entry(component.clone()) {
                Entry::Occupied(existing) => Arc::clone(existing.get()),
                Entry::Vacant(slot) => {
                    let child = Arc::new(MountNode::default());
                    slot.insert(Arc::clone(&child));
                    child
                }
            };
            node = next;
        }

        node
    }

    fn clear_descendants(node: &Arc<MountNode>) {
        node.children.clear();
    }

    /// Overwrite the mount at `path`.
    pub fn set_mount(
        &self,
        path: &Path,
        fs: Box<dyn FileSystem + Send + Sync>,
    ) -> Result<()> {
        let path = self.prepare_path(path)?;
        let node = self.mount_node(&Self::path_components(&path));
        node.mount.insert((), MountedFileSystem { fs: Arc::from(fs) });
        Ok(())
    }

    pub fn add_mount_entries_with_mode(
        &self,
        entries: impl IntoIterator<Item = MountEntry>,
        conflict_mode: ExactMountConflictMode,
    ) -> Result<()> {
        let mut skipped_subtrees = Vec::<PathBuf>::new();

        for entry in entries {
            if skipped_subtrees.iter().any(|prefix| entry.path.starts_with(prefix)) {
                continue;
            }

            let exact_conflict = self.filesystem_at(&entry.path).is_some();
            if exact_conflict {
                match conflict_mode {
                    ExactMountConflictMode::Fail => return Err(FsError::AlreadyExists),
                    ExactMountConflictMode::KeepExisting => {
                        skipped_subtrees.push(entry.path);
                        continue;
                    }
                    ExactMountConflictMode::ReplaceExisting => {
                        let node = self.mount_node(&Self::path_components(&self.prepare_path(&entry.path)?));
                        Self::clear_descendants(&node);
                        node.mount.insert((), MountedFileSystem { fs: entry.fs });
                        continue;
                    }
                }
            }

            self.mount(&entry.path, Box::new(entry.fs))?;
        }

        Ok(())
    }
    pub fn mount_entries(&self) -> Vec<MountEntry> {
        let mut entries = Vec::new();
        Self::collect_mount_entries(&self.root, Path::new("/"), &mut entries);
        entries
    }
}

impl FileSystem for MountFileSystem {
    fn readlink(&self, path: &Path) -> Result<PathBuf> {
        let path = self.prepare_path(path)?;

        if path.as_os_str().is_empty() {
            Err(FsError::NotAFile)
        } else {
            if let Some(node) = self.exact_node(&path)
                && node.fs.is_none()
            {
                return Err(FsError::EntryNotFound);
            }

            match self.resolve_mount(path) {
                Some(resolved) => resolved.fs.readlink(&resolved.delegated_path),
                None => Err(FsError::EntryNotFound),
            }
        }
    }

    fn read_dir(&self, path: &Path) -> Result<ReadDir> {
        let path = self.prepare_path(path)?;

        if let Some(node) = self.exact_node(&path) {
            return self.read_dir_from_exact_node(&node);
        }

        match self.resolve_mount(path.clone()) {
            Some(resolved) => {
                let mut entries = resolved.fs.read_dir(&resolved.delegated_path)?;
                Self::rebase_entries(
                    &mut entries,
                    &resolved.delegated_path,
                    &Path::new("/").join(&path),
                );
                Ok(entries)
            }
            None => Err(FsError::EntryNotFound),
        }
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path)?;

        if path.as_os_str().is_empty() {
            return Ok(());
        }

        if let Some(node) = self.exact_node(&path) {
            return if let Some(fs) = node.fs {
                let result = fs.create_dir(Path::new("/"));

                match result {
                    Ok(()) | Err(FsError::AlreadyExists) => Ok(()),
                    Err(error) if Self::should_fallback_to_synthetic_dir(&error) => Ok(()),
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            };
        }

        match self.resolve_mount(path) {
            Some(resolved) => {
                let result = resolved.fs.create_dir(&resolved.delegated_path);

                if let Err(error) = result
                    && error == FsError::AlreadyExists
                {
                    return Ok(());
                }

                result
            }
            None => Err(FsError::EntryNotFound),
        }
    }

    fn create_symlink(&self, source: &Path, target: &Path) -> Result<()> {
        let target = self.prepare_path(target)?;

        if target.as_os_str().is_empty() {
            return Err(FsError::AlreadyExists);
        }

        if self.exact_node(&target).is_some() {
            return Err(FsError::AlreadyExists);
        }

        match self.resolve_mount(target) {
            Some(resolved) => resolved.fs.create_symlink(source, &resolved.delegated_path),
            None => Err(FsError::EntryNotFound),
        }
    }

    fn remove_dir(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path)?;

        if path.as_os_str().is_empty() {
            return Err(FsError::PermissionDenied);
        }

        if let Some(node) = self.exact_node(&path) {
            return if node.fs.is_some() || node.has_children() {
                Err(FsError::PermissionDenied)
            } else {
                Err(FsError::EntryNotFound)
            };
        }

        match self.resolve_mount(path) {
            Some(resolved) => resolved.fs.remove_dir(&resolved.delegated_path),
            None => Err(FsError::EntryNotFound),
        }
    }

    fn rename<'a>(&'a self, from: &'a Path, to: &'a Path) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let from = self.prepare_path(from)?;
            let to = self.prepare_path(to)?;

            if from.as_os_str().is_empty() {
                return Err(FsError::PermissionDenied);
            }

            if let Some(node) = self.exact_node(&from)
                && (node.fs.is_some() || node.has_children())
            {
                return Err(FsError::PermissionDenied);
            }

            if let Some(node) = self.exact_node(&to)
                && (node.fs.is_some() || node.has_children())
            {
                return Err(FsError::PermissionDenied);
            }

            match (self.resolve_mount(from), self.resolve_mount(to)) {
                (Some(from_mount), Some(to_mount))
                    if from_mount.mount_path == to_mount.mount_path =>
                {
                    from_mount
                        .fs
                        .rename(&from_mount.delegated_path, &to_mount.delegated_path)
                        .await
                }
                (Some(_), Some(_)) => Err(FsError::InvalidInput),
                _ => Err(FsError::EntryNotFound),
            }
        })
    }

    fn metadata(&self, path: &Path) -> Result<Metadata> {
        let path = self.prepare_path(path)?;

        if let Some(node) = self.exact_node(&path) {
            return if let Some(fs) = node.fs {
                fs.metadata(Path::new("/"))
                    .or_else(|error| {
                        if Self::should_fallback_to_synthetic_dir(&error) {
                            Ok(Self::directory_metadata())
                        } else {
                            Err(error)
                        }
                    })
            } else if node.has_children() {
                Ok(Self::directory_metadata())
            } else {
                Err(FsError::EntryNotFound)
            };
        }

        match self.resolve_mount(path) {
            Some(resolved) => resolved.fs.metadata(&resolved.delegated_path),
            None => Err(FsError::EntryNotFound),
        }
    }

    fn symlink_metadata(&self, path: &Path) -> Result<Metadata> {
        let path = self.prepare_path(path)?;

        if let Some(node) = self.exact_node(&path) {
            return if let Some(fs) = node.fs {
                fs.symlink_metadata(Path::new("/"))
                    .or_else(|error| {
                        if Self::should_fallback_to_synthetic_dir(&error) {
                            Ok(Self::directory_metadata())
                        } else {
                            Err(error)
                        }
                    })
            } else if node.has_children() {
                Ok(Self::directory_metadata())
            } else {
                Err(FsError::EntryNotFound)
            };
        }

        match self.resolve_mount(path) {
            Some(resolved) => resolved.fs.symlink_metadata(&resolved.delegated_path),
            None => Err(FsError::EntryNotFound),
        }
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let path = self.prepare_path(path)?;

        if path.as_os_str().is_empty() {
            return Err(FsError::NotAFile);
        }

        if let Some(node) = self.exact_node(&path) {
            return if node.fs.is_some() || node.has_children() {
                Err(FsError::PermissionDenied)
            } else {
                Err(FsError::EntryNotFound)
            };
        }

        match self.resolve_mount(path) {
            Some(resolved) => resolved.fs.remove_file(&resolved.delegated_path),
            None => Err(FsError::EntryNotFound),
        }
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        OpenOptions::new(self)
    }
}

#[derive(Debug)]
pub struct MountPointRef<'a> {
    pub path: PathBuf,
    pub name: String,
    pub fs: Option<&'a (dyn FileSystem + Send + Sync)>,
}

impl FileOpener for MountFileSystem {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> Result<Box<dyn VirtualFile + Send + Sync>> {
        let path = self.prepare_path(path)?;

        if path.as_os_str().is_empty() {
            return Err(FsError::NotAFile);
        }

        if let Some(node) = self.exact_node(&path)
            && node.fs.is_none()
        {
            return Err(FsError::NotAFile);
        }

        match self.resolve_mount(path) {
            Some(resolved) => resolved
                .fs
                .new_open_options()
                .options(conf.clone())
                .open(resolved.delegated_path),
            None => Err(FsError::EntryNotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        path::{Path, PathBuf},
    };

    use tokio::io::AsyncWriteExt;

    use crate::{FileSystem as FileSystemTrait, FsError, MountFileSystem, mem_fs};

    use super::{FileOpener, OpenOptionsConfig};

    #[derive(Debug, Clone, Default)]
    struct MountlessFileSystem {
        inner: mem_fs::FileSystem,
    }

    #[derive(Debug, Clone, Default)]
    struct RootOpaqueFileSystem {
        inner: mem_fs::FileSystem,
    }

    #[derive(Debug, Clone, Default)]
    struct RootPermissionDeniedFileSystem;

    impl FileSystemTrait for MountlessFileSystem {
        fn readlink(&self, path: &Path) -> crate::Result<PathBuf> {
            self.inner.readlink(path)
        }

        fn read_dir(&self, path: &Path) -> crate::Result<crate::ReadDir> {
            self.inner.read_dir(path)
        }

        fn create_dir(&self, path: &Path) -> crate::Result<()> {
            self.inner.create_dir(path)
        }

        fn remove_dir(&self, path: &Path) -> crate::Result<()> {
            self.inner.remove_dir(path)
        }

        fn rename<'a>(
            &'a self,
            from: &'a Path,
            to: &'a Path,
        ) -> futures::future::BoxFuture<'a, crate::Result<()>> {
            Box::pin(async move { self.inner.rename(from, to).await })
        }

        fn metadata(&self, path: &Path) -> crate::Result<crate::Metadata> {
            self.inner.metadata(path)
        }

        fn symlink_metadata(&self, path: &Path) -> crate::Result<crate::Metadata> {
            self.inner.symlink_metadata(path)
        }

        fn remove_file(&self, path: &Path) -> crate::Result<()> {
            self.inner.remove_file(path)
        }

        fn new_open_options(&self) -> crate::OpenOptions<'_> {
            self.inner.new_open_options()
        }
    }

    impl FileOpener for MountlessFileSystem {
        fn open(
            &self,
            path: &Path,
            conf: &OpenOptionsConfig,
        ) -> crate::Result<Box<dyn crate::VirtualFile + Send + Sync>> {
            self.inner
                .new_open_options()
                .options(conf.clone())
                .open(path)
        }
    }

    impl FileSystemTrait for RootOpaqueFileSystem {
        fn readlink(&self, path: &Path) -> crate::Result<PathBuf> {
            self.inner.readlink(path)
        }

        fn read_dir(&self, path: &Path) -> crate::Result<crate::ReadDir> {
            if path == Path::new("/") {
                Err(FsError::Unsupported)
            } else {
                self.inner.read_dir(path)
            }
        }

        fn create_dir(&self, path: &Path) -> crate::Result<()> {
            self.inner.create_dir(path)
        }

        fn remove_dir(&self, path: &Path) -> crate::Result<()> {
            self.inner.remove_dir(path)
        }

        fn rename<'a>(
            &'a self,
            from: &'a Path,
            to: &'a Path,
        ) -> futures::future::BoxFuture<'a, crate::Result<()>> {
            Box::pin(async move { self.inner.rename(from, to).await })
        }

        fn metadata(&self, path: &Path) -> crate::Result<crate::Metadata> {
            if path == Path::new("/") {
                Err(FsError::Unsupported)
            } else {
                self.inner.metadata(path)
            }
        }

        fn symlink_metadata(&self, path: &Path) -> crate::Result<crate::Metadata> {
            if path == Path::new("/") {
                Err(FsError::Unsupported)
            } else {
                self.inner.symlink_metadata(path)
            }
        }

        fn remove_file(&self, path: &Path) -> crate::Result<()> {
            self.inner.remove_file(path)
        }

        fn new_open_options(&self) -> crate::OpenOptions<'_> {
            self.inner.new_open_options()
        }
    }

    impl FileOpener for RootOpaqueFileSystem {
        fn open(
            &self,
            path: &Path,
            conf: &OpenOptionsConfig,
        ) -> crate::Result<Box<dyn crate::VirtualFile + Send + Sync>> {
            self.inner
                .new_open_options()
                .options(conf.clone())
                .open(path)
        }
    }

    impl FileSystemTrait for RootPermissionDeniedFileSystem {
        fn readlink(&self, _path: &Path) -> crate::Result<PathBuf> {
            Err(FsError::PermissionDenied)
        }

        fn read_dir(&self, _path: &Path) -> crate::Result<crate::ReadDir> {
            Err(FsError::PermissionDenied)
        }

        fn create_dir(&self, _path: &Path) -> crate::Result<()> {
            Err(FsError::PermissionDenied)
        }

        fn remove_dir(&self, _path: &Path) -> crate::Result<()> {
            Err(FsError::PermissionDenied)
        }

        fn rename<'a>(
            &'a self,
            _from: &'a Path,
            _to: &'a Path,
        ) -> futures::future::BoxFuture<'a, crate::Result<()>> {
            Box::pin(async { Err(FsError::PermissionDenied) })
        }

        fn metadata(&self, _path: &Path) -> crate::Result<crate::Metadata> {
            Err(FsError::PermissionDenied)
        }

        fn symlink_metadata(&self, _path: &Path) -> crate::Result<crate::Metadata> {
            Err(FsError::PermissionDenied)
        }

        fn remove_file(&self, _path: &Path) -> crate::Result<()> {
            Err(FsError::PermissionDenied)
        }

        fn new_open_options(&self) -> crate::OpenOptions<'_> {
            crate::OpenOptions::new(self)
        }
    }

    impl FileOpener for RootPermissionDeniedFileSystem {
        fn open(
            &self,
            _path: &Path,
            _conf: &OpenOptionsConfig,
        ) -> crate::Result<Box<dyn crate::VirtualFile + Send + Sync>> {
            Err(FsError::PermissionDenied)
        }
    }

    fn gen_filesystem() -> MountFileSystem {
        let union = MountFileSystem::new();
        let a = mem_fs::FileSystem::default();
        let b = mem_fs::FileSystem::default();
        let c = mem_fs::FileSystem::default();
        let d = mem_fs::FileSystem::default();
        let e = mem_fs::FileSystem::default();
        let f = mem_fs::FileSystem::default();
        let g = mem_fs::FileSystem::default();
        let h = mem_fs::FileSystem::default();

        union
            .mount(PathBuf::from("/test_new_filesystem").as_path(), Box::new(a))
            .unwrap();
        union
            .mount(PathBuf::from("/test_create_dir").as_path(), Box::new(b))
            .unwrap();
        union
            .mount(PathBuf::from("/test_remove_dir").as_path(), Box::new(c))
            .unwrap();
        union
            .mount(PathBuf::from("/test_rename").as_path(), Box::new(d))
            .unwrap();
        union
            .mount(PathBuf::from("/test_metadata").as_path(), Box::new(e))
            .unwrap();
        union
            .mount(PathBuf::from("/test_remove_file").as_path(), Box::new(f))
            .unwrap();
        union
            .mount(PathBuf::from("/test_readdir").as_path(), Box::new(g))
            .unwrap();
        union
            .mount(PathBuf::from("/test_canonicalize").as_path(), Box::new(h))
            .unwrap();

        union
    }

    fn gen_nested_filesystem() -> MountFileSystem {
        let union = MountFileSystem::new();
        let a = mem_fs::FileSystem::default();
        a.open(
            &PathBuf::from("/data-a.txt"),
            &OpenOptionsConfig {
                read: true,
                write: true,
                create_new: false,
                create: true,
                append: false,
                truncate: false,
            },
        )
        .unwrap();
        let b = mem_fs::FileSystem::default();
        b.open(
            &PathBuf::from("/data-b.txt"),
            &OpenOptionsConfig {
                read: true,
                write: true,
                create_new: false,
                create: true,
                append: false,
                truncate: false,
            },
        )
        .unwrap();

        union
            .mount(PathBuf::from("/app/a").as_path(), Box::new(a))
            .unwrap();
        union
            .mount(PathBuf::from("/app/b").as_path(), Box::new(b))
            .unwrap();

        union
    }

    #[tokio::test]
    async fn test_nested_read_dir() {
        let fs = gen_nested_filesystem();

        let root_contents: Vec<PathBuf> = fs
            .read_dir(&PathBuf::from("/"))
            .unwrap()
            .map(|e| e.unwrap().path.clone())
            .collect();
        assert_eq!(root_contents, vec![PathBuf::from("/app")]);

        let app_contents: HashSet<PathBuf> = fs
            .read_dir(&PathBuf::from("/app"))
            .unwrap()
            .map(|e| e.unwrap().path)
            .collect();
        assert_eq!(
            app_contents,
            HashSet::from_iter([PathBuf::from("/app/a"), PathBuf::from("/app/b")].into_iter())
        );

        let a_contents: Vec<PathBuf> = fs
            .read_dir(&PathBuf::from("/app/a"))
            .unwrap()
            .map(|e| e.unwrap().path.clone())
            .collect();
        assert_eq!(a_contents, vec![PathBuf::from("/app/a/data-a.txt")]);

        let b_contents: Vec<PathBuf> = fs
            .read_dir(&PathBuf::from("/app/b"))
            .unwrap()
            .map(|e| e.unwrap().path)
            .collect();
        assert_eq!(b_contents, vec![PathBuf::from("/app/b/data-b.txt")]);
    }

    #[tokio::test]
    async fn test_nested_metadata() {
        let fs = gen_nested_filesystem();

        assert!(fs.metadata(&PathBuf::from("/")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app/a")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app/b")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app/a/data-a.txt")).is_ok());
        assert!(fs.metadata(&PathBuf::from("/app/b/data-b.txt")).is_ok());
    }

    #[tokio::test]
    async fn test_nested_symlink_metadata() {
        let fs = gen_nested_filesystem();

        assert!(fs.symlink_metadata(&PathBuf::from("/")).is_ok());
        assert!(fs.symlink_metadata(&PathBuf::from("/app")).is_ok());
        assert!(fs.symlink_metadata(&PathBuf::from("/app/a")).is_ok());
        assert!(fs.symlink_metadata(&PathBuf::from("/app/b")).is_ok());
        assert!(
            fs.symlink_metadata(&PathBuf::from("/app/a/data-a.txt"))
                .is_ok()
        );
        assert!(
            fs.symlink_metadata(&PathBuf::from("/app/b/data-b.txt"))
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_import_mounts_preserves_nested_root_mounts() {
        let primary = MountFileSystem::new();
        let openssl = mem_fs::FileSystem::default();
        openssl.create_dir(Path::new("/certs")).unwrap();
        openssl
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/certs/ca.pem"))
            .unwrap();
        primary
            .mount(Path::new("/openssl"), Box::new(openssl))
            .unwrap();

        let injected = MountFileSystem::new();
        let app = mem_fs::FileSystem::default();
        app.new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/index.php"))
            .unwrap();
        injected
            .mount(Path::new("/app"), Box::new(app))
            .unwrap();

        let assets = mem_fs::FileSystem::default();
        assets.create_dir(Path::new("/css")).unwrap();
        assets
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/css/site.css"))
            .unwrap();
        injected
            .mount(Path::new("/opt/assets"), Box::new(assets))
            .unwrap();

        primary
            .add_mount_entries_with_mode(injected.mount_entries(), super::ExactMountConflictMode::Fail)
            .unwrap();

        let root_contents = read_dir_names(&primary, "/");
        assert!(root_contents.contains(&"app".to_string()));
        assert!(root_contents.contains(&"opt".to_string()));
        assert!(root_contents.contains(&"openssl".to_string()));
        assert!(primary.metadata(Path::new("/app/index.php")).is_ok());
        assert!(
            primary
                .metadata(Path::new("/opt/assets/css/site.css"))
                .is_ok()
        );
        assert!(primary.metadata(Path::new("/openssl/certs/ca.pem")).is_ok());
    }

    #[tokio::test]
    async fn test_nested_mount_under_non_mountable_leaf_is_supported() {
        let fs = MountFileSystem::new();

        let top = MountlessFileSystem::default();
        top.create_dir(Path::new("/bin")).unwrap();
        top.new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/bin/tool"))
            .unwrap();

        let nested = mem_fs::FileSystem::default();
        nested.create_dir(Path::new("/css")).unwrap();
        nested
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/css/site.css"))
            .unwrap();

        fs.mount(Path::new("/opt"), Box::new(top))
            .unwrap();
        fs.mount(
            Path::new("/opt/assets"),
            Box::new(nested),
        )
        .unwrap();

        assert!(fs.metadata(Path::new("/opt/bin/tool")).is_ok());
        assert!(fs.metadata(Path::new("/opt/assets/css/site.css")).is_ok());
    }

    #[tokio::test]
    async fn test_normalized_paths_still_route_to_deepest_mount() {
        let fs = MountFileSystem::new();

        let top = MountlessFileSystem::default();
        top.create_dir(Path::new("/bin")).unwrap();
        top.new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/bin/tool"))
            .unwrap();

        let nested = mem_fs::FileSystem::default();
        nested.create_dir(Path::new("/css")).unwrap();
        nested
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/css/site.css"))
            .unwrap();

        fs.mount(Path::new("/opt"), Box::new(top))
            .unwrap();
        fs.mount(
            Path::new("/opt/assets"),
            Box::new(nested),
        )
        .unwrap();

        assert!(
            fs.metadata(Path::new("/opt/./assets/../assets/css/site.css"))
                .unwrap()
                .is_file()
        );
    }

    #[tokio::test]
    async fn test_invalid_above_root_path_is_rejected() {
        let fs = MountFileSystem::new();
        fs.mount(Path::new("/"), Box::new(mem_fs::FileSystem::default()))
        .unwrap();

        assert_eq!(fs.metadata(Path::new("../foo")), Err(FsError::InvalidInput));
    }

    #[tokio::test]
    async fn test_exact_mount_metadata_falls_back_to_synthetic_directory() {
        let fs = MountFileSystem::new();
        fs.mount(Path::new("/opaque"), Box::new(RootOpaqueFileSystem::default()))
        .unwrap();

        assert!(fs.metadata(Path::new("/opaque")).unwrap().is_dir());
        assert!(fs.symlink_metadata(Path::new("/opaque")).unwrap().is_dir());
        assert_eq!(fs.create_dir(Path::new("/opaque")), Ok(()));
    }

    #[tokio::test]
    async fn test_exact_mount_read_dir_falls_back_to_child_mounts_when_root_is_unlistable() {
        let fs = MountFileSystem::new();
        fs.mount(Path::new("/opaque"), Box::new(RootOpaqueFileSystem::default()))
        .unwrap();
        fs.mount(
            Path::new("/opaque/assets"),
            Box::new(mem_fs::FileSystem::default()),
        )
        .unwrap();

        assert_eq!(read_dir_names(&fs, "/opaque"), vec!["assets".to_string()]);
    }

    #[tokio::test]
    async fn test_exact_mount_fallback_does_not_mask_permission_denied() {
        let fs = MountFileSystem::new();
        fs.mount(Path::new("/denied"), Box::new(RootPermissionDeniedFileSystem))
        .unwrap();
        fs.mount(
            Path::new("/denied/assets"),
            Box::new(mem_fs::FileSystem::default()),
        )
        .unwrap();

        assert_eq!(
            fs.metadata(Path::new("/denied")),
            Err(FsError::PermissionDenied)
        );
        assert_eq!(
            fs.symlink_metadata(Path::new("/denied")),
            Err(FsError::PermissionDenied)
        );
        assert_eq!(
            fs.read_dir(Path::new("/denied")).map(|_| ()),
            Err(FsError::PermissionDenied)
        );
        assert_eq!(
            fs.create_dir(Path::new("/denied")),
            Err(FsError::PermissionDenied)
        );
    }

    #[tokio::test]
    async fn test_keep_existing_conflict_skips_the_other_subtree() {
        let primary = MountFileSystem::new();
        let user_mount = mem_fs::FileSystem::default();
        user_mount
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/user.txt"))
            .unwrap();
        primary
            .mount(Path::new("/python"), Box::new(user_mount))
            .unwrap();

        let injected = MountFileSystem::new();
        let package_mount = mem_fs::FileSystem::default();
        package_mount
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/pkg.txt"))
            .unwrap();
        injected
            .mount(Path::new("/python"), Box::new(package_mount))
            .unwrap();

        let package_child = mem_fs::FileSystem::default();
        package_child
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/child.txt"))
            .unwrap();
        injected
            .mount(Path::new("/python/lib"), Box::new(package_child))
            .unwrap();

        primary
            .add_mount_entries_with_mode(
                injected.mount_entries(),
                super::ExactMountConflictMode::KeepExisting,
            )
            .unwrap();

        assert!(primary.metadata(Path::new("/python/user.txt")).unwrap().is_file());
        assert_eq!(
            primary.metadata(Path::new("/python/pkg.txt")),
            Err(FsError::EntryNotFound)
        );
        assert_eq!(
            primary.metadata(Path::new("/python/lib/child.txt")),
            Err(FsError::EntryNotFound)
        );
    }

    #[tokio::test]
    async fn test_replace_existing_conflict_replaces_the_whole_subtree() {
        let primary = MountFileSystem::new();
        let user_mount = mem_fs::FileSystem::default();
        user_mount
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/user.txt"))
            .unwrap();
        let user_child = mem_fs::FileSystem::default();
        user_child
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/user-child.txt"))
            .unwrap();
        primary
            .mount(Path::new("/python"), Box::new(user_mount))
            .unwrap();
        primary
            .mount(Path::new("/python/lib"), Box::new(user_child))
            .unwrap();

        let injected = MountFileSystem::new();
        let package_mount = mem_fs::FileSystem::default();
        package_mount
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/pkg.txt"))
            .unwrap();
        let package_child = mem_fs::FileSystem::default();
        package_child
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/pkg-child.txt"))
            .unwrap();
        injected
            .mount(Path::new("/python"), Box::new(package_mount))
            .unwrap();
        injected
            .mount(Path::new("/python/lib"), Box::new(package_child))
            .unwrap();

        primary
            .add_mount_entries_with_mode(
                injected.mount_entries(),
                super::ExactMountConflictMode::ReplaceExisting,
            )
            .unwrap();

        assert_eq!(
            primary.metadata(Path::new("/python/user.txt")),
            Err(FsError::EntryNotFound)
        );
        assert_eq!(
            primary.metadata(Path::new("/python/lib/user-child.txt")),
            Err(FsError::EntryNotFound)
        );
        assert!(primary.metadata(Path::new("/python/pkg.txt")).unwrap().is_file());
        assert!(
            primary
                .metadata(Path::new("/python/lib/pkg-child.txt"))
                .unwrap()
                .is_file()
        );
    }

    #[tokio::test]
    async fn test_exact_mountpoints_reject_destructive_mutation() {
        let fs = MountFileSystem::new();
        let mounted = mem_fs::FileSystem::default();
        mounted.create_dir(Path::new("/dir")).unwrap();
        mounted
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/file.txt"))
            .unwrap();

        fs.mount(Path::new("/mounted"), Box::new(mounted))
        .unwrap();

        assert_eq!(
            fs.remove_dir(Path::new("/mounted")),
            Err(FsError::PermissionDenied)
        );
        assert_eq!(
            fs.remove_file(Path::new("/mounted")),
            Err(FsError::PermissionDenied)
        );
        assert_eq!(
            fs.rename(Path::new("/mounted"), Path::new("/other")).await,
            Err(FsError::PermissionDenied)
        );
        assert_eq!(
            fs.rename(Path::new("/mounted/file.txt"), Path::new("/mounted")).await,
            Err(FsError::PermissionDenied)
        );
    }

    #[tokio::test]
    async fn test_parent_read_dir_merges_leaf_entries_with_child_mounts() {
        let fs = MountFileSystem::new();

        let top = MountlessFileSystem::default();
        top.create_dir(Path::new("/bin")).unwrap();
        top.new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/bin/tool"))
            .unwrap();

        let nested = mem_fs::FileSystem::default();
        nested.create_dir(Path::new("/css")).unwrap();
        nested
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/css/site.css"))
            .unwrap();

        fs.mount(Path::new("/opt"), Box::new(top))
            .unwrap();
        fs.mount(
            Path::new("/opt/assets"),
            Box::new(nested),
        )
        .unwrap();

        let opt_contents = read_dir_names(&fs, "/opt");
        assert!(opt_contents.contains(&"bin".to_string()));
        assert!(opt_contents.contains(&"assets".to_string()));
    }

    #[tokio::test]
    async fn test_child_mount_shadows_same_named_parent_entry() {
        let fs = MountFileSystem::new();

        let top = MountlessFileSystem::default();
        top.new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/assets"))
            .unwrap();

        let nested = mem_fs::FileSystem::default();
        nested.create_dir(Path::new("/css")).unwrap();
        nested
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/css/site.css"))
            .unwrap();

        fs.mount(Path::new("/opt"), Box::new(top))
            .unwrap();
        fs.mount(
            Path::new("/opt/assets"),
            Box::new(nested),
        )
        .unwrap();

        assert!(fs.metadata(Path::new("/opt/assets")).unwrap().is_dir());
        assert_eq!(
            read_dir_names(&fs, "/opt")
                .into_iter()
                .filter(|entry| entry == "assets")
                .count(),
            1,
        );
        assert!(fs.metadata(Path::new("/opt/assets/css/site.css")).is_ok());
    }

    #[tokio::test]
    async fn test_read_dir_rebases_entries_under_nested_mount_subdirectory() {
        let fs = MountFileSystem::new();

        let nested = mem_fs::FileSystem::default();
        nested.create_dir(Path::new("/css")).unwrap();
        nested
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/css/site.css"))
            .unwrap();

        fs.mount(Path::new("/opt/assets"), Box::new(nested))
        .unwrap();

        let css_contents: Vec<PathBuf> = fs
            .read_dir(Path::new("/opt/assets/css"))
            .unwrap()
            .map(|entry| entry.unwrap().path)
            .collect();

        assert_eq!(
            css_contents,
            vec![PathBuf::from("/opt/assets/css/site.css")]
        );
    }

    #[tokio::test]
    async fn test_import_mounts_allows_shared_prefix_without_exact_mount_conflict() {
        let primary = MountFileSystem::new();
        let bin = mem_fs::FileSystem::default();
        bin.new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/tool"))
            .unwrap();
        primary
            .mount(Path::new("/opt/bin"), Box::new(bin))
            .unwrap();

        let injected = MountFileSystem::new();
        let assets = mem_fs::FileSystem::default();
        assets
            .new_open_options()
            .write(true)
            .create_new(true)
            .open(Path::new("/logo.svg"))
            .unwrap();
        injected
            .mount(Path::new("/opt/assets"), Box::new(assets))
            .unwrap();

        primary
            .add_mount_entries_with_mode(injected.mount_entries(), super::ExactMountConflictMode::Fail)
            .unwrap();

        assert!(primary.metadata(Path::new("/opt/bin/tool")).is_ok());
        assert!(primary.metadata(Path::new("/opt/assets/logo.svg")).is_ok());
    }

    #[tokio::test]
    async fn test_import_mounts_rejects_exact_mount_conflict() {
        let primary = MountFileSystem::new();
        primary
            .mount(Path::new("/opt/bin"), Box::new(mem_fs::FileSystem::default()))
            .unwrap();

        let injected = MountFileSystem::new();
        injected
            .mount(Path::new("/opt/bin"), Box::new(mem_fs::FileSystem::default()))
            .unwrap();

        assert_eq!(
            primary.add_mount_entries_with_mode(
                injected.mount_entries(),
                super::ExactMountConflictMode::Fail,
            ),
            Err(FsError::AlreadyExists)
        );
    }

    #[tokio::test]
    async fn test_new_filesystem() {
        let fs = gen_filesystem();
        assert!(
            fs.read_dir(Path::new("/test_new_filesystem")).is_ok(),
            "hostfs can read root"
        );
        let mut file_write = fs
            .new_open_options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(Path::new("/test_new_filesystem/foo2.txt"))
            .unwrap();
        file_write.write_all(b"hello").await.unwrap();
        let _ = std::fs::remove_file("/test_new_filesystem/foo2.txt");
    }

    #[tokio::test]
    async fn test_create_dir() {
        let fs = gen_filesystem();

        assert_eq!(fs.create_dir(Path::new("/")), Ok(()));

        assert_eq!(fs.create_dir(Path::new("/test_create_dir")), Ok(()));

        assert_eq!(
            fs.create_dir(Path::new("/test_create_dir/foo")),
            Ok(()),
            "creating a directory",
        );

        let cur_dir = read_dir_names(&fs, "/test_create_dir");

        if !cur_dir.contains(&"foo".to_string()) {
            panic!("cur_dir does not contain foo: {cur_dir:#?}");
        }

        assert!(
            cur_dir.contains(&"foo".to_string()),
            "the root is updated and well-defined"
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_create_dir/foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        let foo_dir = read_dir_names(&fs, "/test_create_dir/foo");

        assert!(
            foo_dir.contains(&"bar".to_string()),
            "the foo directory is updated and well-defined"
        );

        let bar_dir = read_dir_names(&fs, "/test_create_dir/foo/bar");

        assert!(
            bar_dir.is_empty(),
            "the foo directory is updated and well-defined"
        );
        let _ = fs_extra::remove_items(&["/test_create_dir"]);
    }

    #[tokio::test]
    async fn test_remove_dir() {
        let fs = gen_filesystem();

        assert_eq!(
            fs.remove_dir(Path::new("/")),
            Err(FsError::PermissionDenied),
            "cannot remove the root directory",
        );

        assert_eq!(
            fs.remove_dir(Path::new("/foo")),
            Err(FsError::EntryNotFound),
            "cannot remove a directory that doesn't exist",
        );

        assert_eq!(fs.create_dir(Path::new("/test_remove_dir")), Ok(()));

        assert_eq!(
            fs.create_dir(Path::new("/test_remove_dir/foo")),
            Ok(()),
            "creating a directory",
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_remove_dir/foo/bar")),
            Ok(()),
            "creating a sub-directory",
        );

        assert!(
            read_dir_names(&fs, "/test_remove_dir/foo").contains(&"bar".to_string()),
            "./foo/bar exists"
        );

        assert_eq!(
            fs.remove_dir(Path::new("/test_remove_dir/foo")),
            Err(FsError::DirectoryNotEmpty),
            "removing a directory that has children",
        );

        assert_eq!(
            fs.remove_dir(Path::new("/test_remove_dir/foo/bar")),
            Ok(()),
            "removing a sub-directory",
        );

        assert_eq!(
            fs.remove_dir(Path::new("/test_remove_dir/foo")),
            Ok(()),
            "removing a directory",
        );

        assert!(
            !read_dir_names(&fs, "/test_remove_dir").contains(&"foo".to_string()),
            "the foo directory still exists"
        );
    }

    fn read_dir_names(fs: &dyn crate::FileSystem, path: &str) -> Vec<String> {
        fs.read_dir(Path::new(path))
            .unwrap()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_str()?.to_string()))
            .collect::<Vec<_>>()
    }

    #[tokio::test]
    async fn test_rename() {
        let fs = gen_filesystem();

        assert_eq!(
            fs.rename(Path::new("/"), Path::new("/bar")).await,
            Err(FsError::PermissionDenied),
            "renaming a directory that has no parent",
        );
        assert_eq!(
            fs.rename(Path::new("/foo"), Path::new("/")).await,
            Err(FsError::PermissionDenied),
            "renaming to the synthetic root directory is rejected",
        );

        assert_eq!(fs.create_dir(Path::new("/test_rename")), Ok(()));
        assert_eq!(fs.create_dir(Path::new("/test_rename/foo")), Ok(()));
        assert_eq!(fs.create_dir(Path::new("/test_rename/foo/qux")), Ok(()));

        assert_eq!(
            fs.rename(
                Path::new("/test_rename/foo"),
                Path::new("/test_rename/bar/baz")
            )
            .await,
            Err(FsError::EntryNotFound),
            "renaming to a directory that has parent that doesn't exist",
        );

        assert_eq!(fs.create_dir(Path::new("/test_rename/bar")), Ok(()));

        assert_eq!(
            fs.rename(Path::new("/test_rename/foo"), Path::new("/test_rename/bar"))
                .await,
            Ok(()),
            "renaming to a directory that has parent that exists",
        );

        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_rename/bar/hello1.txt"))
                .is_ok(),
            "creating a new file (`hello1.txt`)",
        );
        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_rename/bar/hello2.txt"))
                .is_ok(),
            "creating a new file (`hello2.txt`)",
        );

        let cur_dir = read_dir_names(&fs, "/test_rename");

        assert!(
            !cur_dir.contains(&"foo".to_string()),
            "the foo directory still exists"
        );

        assert!(
            cur_dir.contains(&"bar".to_string()),
            "the bar directory still exists"
        );

        let bar_dir = read_dir_names(&fs, "/test_rename/bar");

        if !bar_dir.contains(&"qux".to_string()) {
            println!("qux does not exist: {bar_dir:?}")
        }

        let qux_dir = read_dir_names(&fs, "/test_rename/bar/qux");

        assert!(qux_dir.is_empty(), "the qux directory is empty");

        assert!(
            read_dir_names(&fs, "/test_rename/bar").contains(&"hello1.txt".to_string()),
            "the /bar/hello1.txt file exists"
        );

        assert!(
            read_dir_names(&fs, "/test_rename/bar").contains(&"hello2.txt".to_string()),
            "the /bar/hello2.txt file exists"
        );

        assert_eq!(
            fs.create_dir(Path::new("/test_rename/foo")),
            Ok(()),
            "create ./foo again",
        );

        assert_eq!(
            fs.rename(
                Path::new("/test_rename/bar/hello2.txt"),
                Path::new("/test_rename/foo/world2.txt")
            )
            .await,
            Ok(()),
            "renaming (and moving) a file",
        );

        assert_eq!(
            fs.rename(
                Path::new("/test_rename/foo"),
                Path::new("/test_rename/bar/baz")
            )
            .await,
            Ok(()),
            "renaming a directory",
        );

        assert_eq!(
            fs.rename(
                Path::new("/test_rename/bar/hello1.txt"),
                Path::new("/test_rename/bar/world1.txt")
            )
            .await,
            Ok(()),
            "renaming a file (in the same directory)",
        );

        assert!(
            read_dir_names(&fs, "/test_rename").contains(&"bar".to_string()),
            "./bar exists"
        );

        assert!(
            read_dir_names(&fs, "/test_rename/bar").contains(&"baz".to_string()),
            "/bar/baz exists"
        );
        assert!(
            !read_dir_names(&fs, "/test_rename").contains(&"foo".to_string()),
            "foo does not exist anymore"
        );
        assert!(
            read_dir_names(&fs, "/test_rename/bar/baz").contains(&"world2.txt".to_string()),
            "/bar/baz/world2.txt exists"
        );
        assert!(
            read_dir_names(&fs, "/test_rename/bar").contains(&"world1.txt".to_string()),
            "/bar/world1.txt (ex hello1.txt) exists"
        );
        assert!(
            !read_dir_names(&fs, "/test_rename/bar").contains(&"hello1.txt".to_string()),
            "hello1.txt was moved"
        );
        assert!(
            !read_dir_names(&fs, "/test_rename/bar").contains(&"hello2.txt".to_string()),
            "hello2.txt was moved"
        );
        assert!(
            read_dir_names(&fs, "/test_rename/bar/baz").contains(&"world2.txt".to_string()),
            "world2.txt was moved to the correct place"
        );

        let _ = fs_extra::remove_items(&["/test_rename"]);
    }

    #[tokio::test]
    async fn test_metadata() {
        use std::thread::sleep;
        use std::time::Duration;

        let fs = gen_filesystem();

        let root_metadata = fs.metadata(Path::new("/test_metadata")).unwrap();

        assert!(root_metadata.ft.dir);
        assert_eq!(root_metadata.accessed, root_metadata.created);
        assert_eq!(root_metadata.modified, root_metadata.created);
        assert!(root_metadata.modified > 0);

        assert_eq!(fs.create_dir(Path::new("/test_metadata/foo")), Ok(()));

        let foo_metadata = fs.metadata(Path::new("/test_metadata/foo"));
        assert!(foo_metadata.is_ok());
        let foo_metadata = foo_metadata.unwrap();

        assert!(foo_metadata.ft.dir);
        assert!(foo_metadata.accessed == foo_metadata.created);
        assert!(foo_metadata.modified == foo_metadata.created);
        assert!(foo_metadata.modified > 0);

        sleep(Duration::from_secs(3));

        assert_eq!(
            fs.rename(
                Path::new("/test_metadata/foo"),
                Path::new("/test_metadata/bar")
            )
            .await,
            Ok(())
        );

        let bar_metadata = fs.metadata(Path::new("/test_metadata/bar")).unwrap();
        assert!(bar_metadata.ft.dir);
        assert!(bar_metadata.accessed == foo_metadata.accessed);
        assert!(bar_metadata.created == foo_metadata.created);
        assert!(bar_metadata.modified > foo_metadata.modified);

        let root_metadata = fs.metadata(Path::new("/test_metadata/bar")).unwrap();
        assert!(
            root_metadata.modified > foo_metadata.modified,
            "the parent modified time was updated"
        );

        let _ = fs_extra::remove_items(&["/test_metadata"]);
    }

    #[tokio::test]
    async fn test_remove_file() {
        let fs = gen_filesystem();

        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_remove_file/foo.txt"))
                .is_ok(),
            "creating a new file",
        );

        assert!(read_dir_names(&fs, "/test_remove_file").contains(&"foo.txt".to_string()));

        assert_eq!(
            fs.remove_file(Path::new("/test_remove_file/foo.txt")),
            Ok(()),
            "removing a file that exists",
        );

        assert!(!read_dir_names(&fs, "/test_remove_file").contains(&"foo.txt".to_string()));

        assert_eq!(
            fs.remove_file(Path::new("/test_remove_file/foo.txt")),
            Err(FsError::EntryNotFound),
            "removing a file that doesn't exists",
        );

        let _ = fs_extra::remove_items(&["./test_remove_file"]);
    }

    #[tokio::test]
    async fn test_readdir() {
        let fs = gen_filesystem();

        assert_eq!(
            fs.create_dir(Path::new("/test_readdir/foo")),
            Ok(()),
            "creating `foo`"
        );
        assert_eq!(
            fs.create_dir(Path::new("/test_readdir/foo/sub")),
            Ok(()),
            "creating `sub`"
        );
        assert_eq!(
            fs.create_dir(Path::new("/test_readdir/bar")),
            Ok(()),
            "creating `bar`"
        );
        assert_eq!(
            fs.create_dir(Path::new("/test_readdir/baz")),
            Ok(()),
            "creating `bar`"
        );
        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_readdir/a.txt"))
                .is_ok(),
            "creating `a.txt`",
        );
        assert!(
            fs.new_open_options()
                .write(true)
                .create_new(true)
                .open(Path::new("/test_readdir/b.txt"))
                .is_ok(),
            "creating `b.txt`",
        );

        println!("fs: {fs:?}");

        let readdir = fs.read_dir(Path::new("/test_readdir"));

        assert!(readdir.is_ok(), "reading the directory `/test_readdir/`");

        let mut readdir = readdir.unwrap();

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("foo"), "checking entry #1");
        println!("entry 1: {next:#?}");
        assert!(next.file_type().unwrap().is_dir(), "checking entry #1");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("bar"), "checking entry #2");
        assert!(next.file_type().unwrap().is_dir(), "checking entry #2");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("baz"), "checking entry #3");
        assert!(next.file_type().unwrap().is_dir(), "checking entry #3");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("a.txt"), "checking entry #2");
        assert!(next.file_type().unwrap().is_file(), "checking entry #4");

        let next = readdir.next().unwrap().unwrap();
        assert!(next.path.ends_with("b.txt"), "checking entry #2");
        assert!(next.file_type().unwrap().is_file(), "checking entry #5");

        if let Some(s) = readdir.next() {
            panic!("next: {s:?}");
        }

        let _ = fs_extra::remove_items(&["./test_readdir"]);
    }

    /*
    #[tokio::test]
    async fn test_canonicalize() {
        let fs = gen_filesystem();

        let root_dir = env!("CARGO_MANIFEST_DIR");

        let _ = fs_extra::remove_items(&["./test_canonicalize"]);

        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize")),
            Ok(()),
            "creating `test_canonicalize`"
        );

        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo")),
            Ok(()),
            "creating `foo`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar")),
            Ok(()),
            "creating `bar`"
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar/baz")),
            Ok(()),
            "creating `baz`",
        );
        assert_eq!(
            fs.create_dir(Path::new("./test_canonicalize/foo/bar/baz/qux")),
            Ok(()),
            "creating `qux`",
        );
        assert!(
            matches!(
                fs.new_open_options()
                    .write(true)
                    .create_new(true)
                    .open(Path::new("./test_canonicalize/foo/bar/baz/qux/hello.txt")),
                Ok(_)
            ),
            "creating `hello.txt`",
        );

        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize")).to_path_buf()),
            "canonicalizing `/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("foo")),
            Err(FsError::InvalidInput),
            "canonicalizing `foo`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/././././foo/")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo")).to_path_buf()),
            "canonicalizing `/././././foo/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar//")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar")).to_path_buf()),
            "canonicalizing `/foo/bar//`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar/../bar")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar")).to_path_buf()),
            "canonicalizing `/foo/bar/../bar`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("./test_canonicalize/foo/bar/../..")),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize")).to_path_buf()),
            "canonicalizing `/foo/bar/../..`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("/foo/bar/../../..")),
            Err(FsError::InvalidInput),
            "canonicalizing `/foo/bar/../../..`",
        );
        assert_eq!(
            fs.canonicalize(Path::new("C:/foo/")),
            Err(FsError::InvalidInput),
            "canonicalizing `C:/foo/`",
        );
        assert_eq!(
            fs.canonicalize(Path::new(
                "./test_canonicalize/foo/./../foo/bar/../../foo/bar/./baz/./../baz/qux/../../baz/./qux/hello.txt"
            )),
            Ok(Path::new(&format!("{root_dir}/test_canonicalize/foo/bar/baz/qux/hello.txt")).to_path_buf()),
            "canonicalizing a crazily stupid path name",
        );

        let _ = fs_extra::remove_items(&["./test_canonicalize"]);
    }
    */
}
