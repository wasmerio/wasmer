#![allow(unused)]

use crate::common::normalize_path;
use std::path::{Path, PathBuf};

/// Convenience function that will unpack .tar.gz files and .tar.bz
/// files to a target directory (does NOT remove the original .tar.gz)
pub fn try_unpack_targz<P: AsRef<Path>>(
    target_targz_path: P,
    target_path: P,
    strip_toplevel: bool,
) -> Result<PathBuf, anyhow::Error> {
    let target_targz_path = target_targz_path.as_ref().to_string_lossy().to_string();
    let target_targz_path = normalize_path(&target_targz_path);
    let target_targz_path = Path::new(&target_targz_path);

    let target_path = target_path.as_ref().to_string_lossy().to_string();
    let target_path = normalize_path(&target_path);
    let target_path = Path::new(&target_path);

    let open_file = || {
        std::fs::File::open(target_targz_path)
            .map_err(|e| anyhow::anyhow!("failed to open {}: {e}", target_targz_path.display()))
    };

    let try_decode_gz = || {
        let file = open_file()?;
        let gz_decoded = flate2::read::GzDecoder::new(&file);
        let ar = tar::Archive::new(gz_decoded);
        if strip_toplevel {
            unpack_sans_parent(ar, target_path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to unpack (gz_ar_unpack_sans_parent) {}: {e}",
                    target_targz_path.display()
                )
            })
        } else {
            unpack_with_parent(ar, target_path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to unpack (gz_ar_unpack) {}: {e}",
                    target_targz_path.display()
                )
            })
        }
    };

    let try_decode_xz = || {
        let file = open_file()?;
        let mut decomp: Vec<u8> = Vec::new();
        let mut bufread = std::io::BufReader::new(&file);
        lzma_rs::xz_decompress(&mut bufread, &mut decomp).map_err(|e| {
            anyhow::anyhow!(
                "failed to unpack (try_decode_xz) {}: {e}",
                target_targz_path.display()
            )
        })?;

        let cursor = std::io::Cursor::new(decomp);
        let mut ar = tar::Archive::new(cursor);
        if strip_toplevel {
            unpack_sans_parent(ar, target_path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to unpack (sans parent) {}: {e}",
                    target_targz_path.display()
                )
            })
        } else {
            ar.unpack(target_path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to unpack (with parent) {}: {e}",
                    target_targz_path.display()
                )
            })
        }
    };

    try_decode_gz().or_else(|e1| {
        try_decode_xz()
            .map_err(|e2| anyhow::anyhow!("could not decode gz: {e1}, could not decode xz: {e2}"))
    })?;

    Ok(Path::new(&target_targz_path).to_path_buf())
}

pub fn unpack_with_parent<R>(mut archive: tar::Archive<R>, dst: &Path) -> Result<(), anyhow::Error>
where
    R: std::io::Read,
{
    use std::path::Component::Normal;

    let dst_normalized = normalize_path(dst.to_string_lossy().as_ref());

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path: PathBuf = entry
            .path()?
            .components()
            .skip(1) // strip top-level directory
            .filter(|c| matches!(c, Normal(_))) // prevent traversal attacks
            .collect();
        if entry.header().entry_type().is_file() {
            entry.unpack_in(&dst_normalized)?;
        } else if entry.header().entry_type() == tar::EntryType::Directory {
            std::fs::create_dir_all(Path::new(&dst_normalized).join(&path))?;
        }
    }
    Ok(())
}

pub fn unpack_sans_parent<R>(mut archive: tar::Archive<R>, dst: &Path) -> std::io::Result<()>
where
    R: std::io::Read,
{
    use std::path::Component::Normal;

    let dst_normalized = normalize_path(dst.to_string_lossy().as_ref());

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path: PathBuf = entry
            .path()?
            .components()
            .skip(1) // strip top-level directory
            .filter(|c| matches!(c, Normal(_))) // prevent traversal attacks
            .collect();
        entry.unpack(Path::new(&dst_normalized).join(path))?;
    }
    Ok(())
}
