use std::path::{Component, Path, PathBuf};

pub(crate) fn resolve_path_within(
    root: &Path,
    base: &Path,
    target: &Path,
    normalize: impl Fn(&Path) -> PathBuf,
) -> Option<PathBuf> {
    let root = normalize(root);
    let base = normalize(base);
    if !base.starts_with(&root) {
        return None;
    }

    let (mut resolved, target) = if target.is_absolute() {
        let stripped = if root == Path::new("/") {
            target.strip_prefix(Path::new("/")).ok()?
        } else {
            target.strip_prefix(&root).ok()?
        };
        (root.clone(), stripped)
    } else {
        (base, target)
    };

    for component in target.components() {
        match component {
            Component::Prefix(..) | Component::RootDir => return None,
            Component::CurDir => {}
            Component::ParentDir => {
                if resolved == root {
                    if root.parent().is_none() {
                        continue;
                    }
                    return None;
                }
                if !resolved.pop() || !resolved.starts_with(&root) {
                    return None;
                }
            }
            Component::Normal(part) => {
                resolved.push(part);
                if !resolved.starts_with(&root) {
                    return None;
                }
            }
        }
    }

    Some(resolved)
}
