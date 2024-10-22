use std::{
    collections::BTreeMap,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use webc::{
    v3::{self, write::FileEntry},
    PathSegment, PathSegments,
};

use crate::package::Strictness;

use super::{abstract_volume::Metadata, WasmerPackageVolume};

/// An in-memory representation of a volume.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryVolume {
    /// The internal node
    pub node: MemoryDir,
}

impl MemoryVolume {
    /// The name of the volume used to store metadata files.
    pub(crate) const METADATA: &'static str = "metadata";
}

/// An in-memory representation of a filesystem node.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MemoryNode {
    /// A file
    File(MemoryFile),

    /// A directory
    Dir(MemoryDir),
}

impl MemoryNode {
    /// Try to return a [`MemoryDir`] out of [`self`].
    pub fn as_dir(&self) -> Option<&MemoryDir> {
        match self {
            MemoryNode::Dir(d) => Some(d),
            _ => None,
        }
    }

    /// Try to return a [`MemoryFile`] out of [`self`].
    pub fn as_file(&self) -> Option<&MemoryFile> {
        match self {
            MemoryNode::File(f) => Some(f),
            _ => None,
        }
    }

    fn as_dir_entry(&self) -> anyhow::Result<webc::v3::write::DirEntry<'_>> {
        match self {
            MemoryNode::File(f) => f.as_dir_entry(),
            MemoryNode::Dir(d) => d.as_dir_entry(),
        }
    }

    fn metadata(&self) -> Metadata {
        match self {
            MemoryNode::File(f) => f.metadata(),
            MemoryNode::Dir(d) => d.metadata(),
        }
    }
}

/// An in-memory file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryFile {
    /// When the file was last modified.
    pub modified: SystemTime,
    /// Raw data  
    pub data: Vec<u8>,
}
impl MemoryFile {
    fn as_dir_entry(&self) -> anyhow::Result<v3::write::DirEntry<'_>> {
        Ok(v3::write::DirEntry::File(FileEntry::owned(
            self.data.clone(),
            v3::Timestamps {
                modified: self.modified,
            },
        )))
    }

    fn metadata(&self) -> Metadata {
        let modified = self.modified.duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
        Metadata::File {
            length: self.data.len(),
            timestamps: Some(webc::Timestamps::from_modified(modified)),
        }
    }
}

/// An in-memory directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryDir {
    /// When the directory or its contents were last modified.
    pub modified: SystemTime,
    /// List of nodes in the directory
    pub nodes: BTreeMap<String, MemoryNode>,
}

impl MemoryDir {
    fn metadata(&self) -> Metadata {
        let modified = self.modified.duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
        Metadata::Dir {
            timestamps: Some(webc::Timestamps::from_modified(modified)),
        }
    }

    // Can't return a reference to MemoryNode as it can return itself.
    fn find_node(&self, path: &PathSegments) -> Option<MemoryNode> {
        let mut segments = path.iter().collect::<Vec<_>>();
        if segments.is_empty() {
            return Some(MemoryNode::Dir(self.clone()));
        }

        let mut dir = self;

        while !segments.is_empty() {
            let next = segments.first().unwrap().clone();
            segments.remove(0);

            if let Some(next_node) = dir.nodes.get(&next.to_string()) {
                if segments.is_empty() {
                    return Some(next_node.clone());
                } else {
                    match next_node {
                        MemoryNode::File(_) => break,
                        MemoryNode::Dir(d) => dir = d,
                    }
                }
            }
        }

        None
    }

    fn read_file(&self, path: &PathSegments) -> Option<shared_buffer::OwnedBuffer> {
        self.find_node(path).and_then(|n| {
            if let MemoryNode::File(f) = n {
                Some(shared_buffer::OwnedBuffer::from_bytes(f.data.clone()))
            } else {
                None
            }
        })
    }

    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        self.find_node(path).and_then(|n| {
            if let MemoryNode::Dir(d) = n {
                let mut ret = vec![];

                for (name, node) in &d.nodes {
                    let meta = node.metadata();
                    ret.push((PathSegment::from_str(name).ok()?, None, meta))
                }

                Some(ret)
            } else {
                None
            }
        })
    }

    fn find_meta(&self, path: &PathSegments) -> Option<Metadata> {
        self.find_node(path).map(|n| n.metadata())
    }

    fn as_directory_tree(
        &self,
        _strictness: Strictness,
    ) -> Result<webc::v3::write::Directory<'_>, anyhow::Error> {
        let mut children = BTreeMap::new();

        for (key, value) in self.nodes.iter() {
            children.insert(PathSegment::from_str(key)?, value.as_dir_entry()?);
        }

        let dir = v3::write::Directory::new(
            children,
            v3::Timestamps {
                modified: self.modified,
            },
        );

        Ok(dir)
    }

    fn as_dir_entry(&self) -> anyhow::Result<v3::write::DirEntry<'_>> {
        Ok(v3::write::DirEntry::Dir(
            self.as_directory_tree(Strictness::default())?,
        ))
    }
}

impl WasmerPackageVolume for MemoryVolume {
    fn read_file(&self, path: &PathSegments) -> Option<shared_buffer::OwnedBuffer> {
        self.node.read_file(path)
    }

    fn read_dir(
        &self,
        path: &PathSegments,
    ) -> Option<Vec<(PathSegment, Option<[u8; 32]>, Metadata)>> {
        self.node.read_dir(path)
    }

    fn metadata(&self, path: &PathSegments) -> Option<Metadata> {
        self.node.find_meta(path)
    }

    fn as_directory_tree(
        &self,
        strictness: Strictness,
    ) -> Result<webc::v3::write::Directory<'_>, anyhow::Error> {
        let res = self.node.as_directory_tree(strictness);
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_metadata() -> anyhow::Result<()> {
        let file_modified = SystemTime::now();
        let file_data = String::from("Hello, world!").as_bytes().to_vec();
        let file_data_len = file_data.len();

        let file = MemoryFile {
            modified: file_modified,
            data: file_data,
        };

        let mut nodes = BTreeMap::new();
        nodes.insert(String::from("hello.txt"), MemoryNode::File(file));

        let dir_modified = SystemTime::now();
        let dir = MemoryDir {
            modified: dir_modified,
            nodes,
        };

        let volume = MemoryVolume { node: dir };

        let file_metadata = volume.metadata(&PathSegments::from_str("hello.txt")?);
        assert!(file_metadata.is_some());

        let file_metadata = file_metadata.unwrap();
        assert!(file_metadata.is_file());

        let (length, timestamps) = match file_metadata {
            Metadata::File { length, timestamps } => (length, timestamps),
            _ => unreachable!(),
        };

        assert_eq!(
            timestamps.unwrap().modified(),
            file_modified.duration_since(UNIX_EPOCH)?.as_nanos() as u64
        );

        assert_eq!(length, file_data_len);

        let dir_metadata = volume.metadata(&PathSegments::from_str("/")?);
        assert!(dir_metadata.is_some());

        let dir_metadata = dir_metadata.unwrap();
        assert!(dir_metadata.is_dir());

        let timestamps = match dir_metadata {
            Metadata::Dir { timestamps } => timestamps,
            _ => unreachable!(),
        };

        assert_eq!(
            timestamps.unwrap().modified(),
            dir_modified.duration_since(UNIX_EPOCH)?.as_nanos() as u64
        );

        Ok(())
    }
}
