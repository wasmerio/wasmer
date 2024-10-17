use bytes::Bytes;
use shared_buffer::OwnedBuffer;
use std::{
    cmp,
    fs::File,
    io,
    ops::Range,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::limiter::DynFsMemoryLimiter;

#[derive(Debug)]
pub enum FileExtent {
    MmapOffload { offset: u64, size: u64 },
    RepeatingBytes { value: u8, cnt: u64 },
    InMemory { data: Bytes },
}

impl FileExtent {
    pub fn size(&self) -> u64 {
        match self {
            FileExtent::MmapOffload { size, .. } => *size,
            FileExtent::RepeatingBytes { cnt, .. } => *cnt,
            FileExtent::InMemory { data } => data.len() as u64,
        }
    }

    pub fn resize(&mut self, new_size: u64) {
        match self {
            FileExtent::MmapOffload { size, .. } => *size = new_size.min(*size),
            FileExtent::RepeatingBytes { cnt, .. } => *cnt = new_size,
            FileExtent::InMemory { data } => {
                *data = data.slice(..(new_size as usize));
            }
        }
    }
}

#[derive(Debug)]
struct OffloadBackingStoreState {
    mmap_file: Option<File>,
    mmap_offload: OwnedBuffer,
}

impl OffloadBackingStoreState {
    fn get_slice(&mut self, range: Range<u64>) -> io::Result<&[u8]> {
        let offset = range.start;
        let size = match range.end {
            u64::MAX => {
                let len = self.mmap_offload.len() as u64;
                if len < offset {
                    tracing::trace!("range out of bounds {} vs {}", len, offset);
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                len - offset
            }
            end => end - offset,
        };

        let end = offset + size;
        if end > self.mmap_offload.len() as u64 {
            let mmap_file = match self.mmap_file.as_ref() {
                Some(a) => a,
                None => {
                    tracing::trace!(
                        "mmap buffer out of bounds and no mmap file to reload {} vs {}",
                        end,
                        self.mmap_offload.len()
                    );
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
            };
            self.mmap_offload = OwnedBuffer::from_file(mmap_file)
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            if end > self.mmap_offload.len() as u64 {
                tracing::trace!(
                    "mmap buffer out of bounds {} vs {} for {:?}",
                    end,
                    self.mmap_offload.len(),
                    range
                );
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
        }
        Ok(&self.mmap_offload[offset as usize..end as usize])
    }
}

#[derive(Debug, Clone)]
pub struct OffloadBackingStore {
    state: Arc<Mutex<OffloadBackingStoreState>>,
}

impl OffloadBackingStore {
    pub fn new(mmap_offload: OwnedBuffer, mmap_file: Option<File>) -> Self {
        Self {
            state: Arc::new(Mutex::new(OffloadBackingStoreState {
                mmap_file,
                mmap_offload,
            })),
        }
    }

    pub fn from_file(file: &File) -> Self {
        let file = file.try_clone().unwrap();
        let buffer = OwnedBuffer::from_file(&file).unwrap();
        Self::new(buffer, Some(file))
    }

    pub fn from_buffer(buffer: OwnedBuffer) -> Self {
        Self::new(buffer, None)
    }

    pub fn owned_buffer(&self) -> OwnedBuffer {
        let guard = self.state.lock().unwrap();
        guard.mmap_offload.clone()
    }

    fn lock(&self) -> MutexGuard<'_, OffloadBackingStoreState> {
        self.state.lock().unwrap()
    }
}

#[derive(Debug)]
pub struct OffloadedFile {
    backing: OffloadBackingStore,
    #[allow(dead_code)]
    limiter: Option<DynFsMemoryLimiter>,
    extents: Vec<FileExtent>,
    size: u64,
}

pub enum OffloadWrite<'a> {
    MmapOffset { offset: u64, size: u64 },
    Buffer(&'a [u8]),
}

impl<'a> OffloadWrite<'a> {
    fn len(&self) -> usize {
        match self {
            OffloadWrite::MmapOffset { size, .. } => *size as usize,
            OffloadWrite::Buffer(data) => data.len(),
        }
    }
}

impl OffloadedFile {
    pub fn new(limiter: Option<DynFsMemoryLimiter>, backing: OffloadBackingStore) -> Self {
        Self {
            backing,
            limiter,
            extents: Vec::new(),
            size: 0,
        }
    }

    pub fn seek(&self, position: io::SeekFrom, cursor: &mut u64) -> io::Result<u64> {
        let to_err = |_| io::ErrorKind::InvalidInput;

        // Calculate the next cursor.
        let next_cursor: i64 = match position {
            io::SeekFrom::Start(offset) => offset.try_into().map_err(to_err)?,
            io::SeekFrom::End(offset) => self.len() as i64 + offset,
            io::SeekFrom::Current(offset) => {
                TryInto::<i64>::try_into(*cursor).map_err(to_err)? + offset
            }
        };

        // It's an error to seek before byte 0.
        if next_cursor < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seeking before the byte 0",
            ));
        }

        // In this implementation, it's an error to seek beyond the
        // end of the buffer.
        let next_cursor = next_cursor.try_into().map_err(to_err)?;
        *cursor = cmp::min(self.len(), next_cursor);
        Ok(*cursor)
    }

    pub fn read(&self, mut buf: &mut [u8], cursor: &mut u64) -> io::Result<usize> {
        let cursor_start = *cursor;

        let mut extent_offset = cursor_start;
        let mut extent_index = 0usize;
        while !buf.is_empty() && extent_index < self.extents.len() {
            let extent = &self.extents[extent_index];

            if extent_offset >= extent.size() {
                extent_offset -= extent.size();
                extent_index += 1;
                continue;
            }

            let read = match extent {
                FileExtent::MmapOffload {
                    offset: mmap_offset,
                    size: extent_size,
                } => {
                    let mut backing = self.backing.lock();
                    let mmap_offset_plus_extent = mmap_offset + extent_offset;
                    let data = backing.get_slice(
                        mmap_offset_plus_extent
                            ..(mmap_offset_plus_extent + *extent_size - extent_offset),
                    )?;
                    let data_len = cmp::min(buf.len(), data.len());
                    buf[..data_len].copy_from_slice(&data[..data_len]);
                    data_len
                }
                FileExtent::RepeatingBytes { value, cnt } => {
                    let cnt = cmp::min(buf.len() as u64, cnt - extent_offset) as usize;
                    buf[..cnt].iter_mut().for_each(|d| {
                        *d = *value;
                    });
                    cnt
                }
                FileExtent::InMemory { data } => {
                    let data = &data.as_ref()[extent_offset as usize..];
                    let data_len = cmp::min(buf.len(), data.len());
                    buf[..data_len].copy_from_slice(&data[..data_len]);
                    data_len
                }
            };

            *cursor += read as u64;
            extent_offset = 0;
            extent_index += 1;
            buf = &mut buf[read..];
        }
        Ok((*cursor - cursor_start) as usize)
    }

    pub fn write(&mut self, data: OffloadWrite<'_>, cursor: &mut u64) -> io::Result<usize> {
        let original_extent_offset = *cursor;
        let mut extent_offset = original_extent_offset;
        let mut data_len = data.len() as u64;

        // We need to split any extents that are intersecting with the
        // start or end of the new block of data we are about to write
        let mut split_extents = |mut split_at: u64| {
            let mut index = 0usize;
            while split_at > 0 && index < self.extents.len() {
                let extent = &mut self.extents[index];
                if split_at >= extent.size() {
                    split_at -= extent.size();
                    index += 1;
                    continue;
                } else if split_at == 0 {
                    break;
                } else {
                    let new_extent = match extent {
                        FileExtent::MmapOffload {
                            offset: other_offset,
                            size: other_size,
                        } => FileExtent::MmapOffload {
                            offset: *other_offset + split_at,
                            size: *other_size - split_at,
                        },
                        FileExtent::RepeatingBytes {
                            value: other_value,
                            cnt: other_cnt,
                        } => FileExtent::RepeatingBytes {
                            value: *other_value,
                            cnt: *other_cnt - split_at,
                        },
                        FileExtent::InMemory { data: other_data } => FileExtent::InMemory {
                            data: other_data.slice((split_at as usize)..),
                        },
                    };
                    extent.resize(split_at);
                    self.extents.insert(index + 1, new_extent);
                    break;
                }
            }
        };

        // If the extent is below the actual size of the file then we need to split it
        let mut index = if extent_offset < self.size {
            split_extents(extent_offset);
            split_extents(extent_offset + data_len);

            // Now we delete all the extents that exist between the
            // range that we are about to insert
            let mut index = 0usize;
            while index < self.extents.len() {
                let extent = &self.extents[index];
                if extent_offset >= extent.size() {
                    extent_offset -= extent.size();
                    index += 1;
                    continue;
                } else {
                    break;
                }
            }
            while index < self.extents.len() {
                let extent = &self.extents[index];
                if data_len < extent.size() {
                    break;
                }
                data_len -= extent.size();
                self.extents.remove(index);
            }
            index
        } else {
            self.extents.len()
        };

        // If we have a gap that needs to be filled then do so
        if extent_offset > self.size {
            self.extents.insert(
                index,
                FileExtent::RepeatingBytes {
                    value: 0,
                    cnt: extent_offset - self.size,
                },
            );
            self.size = extent_offset;
            index += 1;
        }

        // Insert the extent into the buffer
        match data {
            OffloadWrite::MmapOffset { offset, size } => {
                self.extents
                    .insert(index, FileExtent::MmapOffload { offset, size });
            }
            OffloadWrite::Buffer(data) => {
                // Finally we add the new extent
                let data_start = data.as_ptr() as u64;
                let data_end = data_start + data.len() as u64;
                let mut backing = self.backing.lock();
                let backing_data = backing.get_slice(0u64..u64::MAX)?;

                let mmap_start = backing_data.as_ptr() as u64;
                let mmap_end = mmap_start + backing_data.len() as u64;

                // If the data is within the mmap buffer then we use a extent range
                // to represent the data, otherwise we fall back on copying the data
                let new_extent = if data_start >= mmap_start && data_end <= mmap_end {
                    FileExtent::MmapOffload {
                        offset: data_start - mmap_start,
                        size: data_end - data_start,
                    }
                } else {
                    FileExtent::InMemory {
                        data: data.to_vec().into(),
                    }
                };
                self.extents.insert(index, new_extent);
            }
        }
        self.size = self.size.max(original_extent_offset + data.len() as u64);

        // Update the cursor
        *cursor += data.len() as u64;
        Ok(data.len())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    pub fn resize(&mut self, new_len: u64, value: u8) {
        let mut cur_len = self.len();
        if new_len > cur_len {
            self.extents.push(FileExtent::RepeatingBytes {
                value,
                cnt: new_len - cur_len,
            });
        }
        while cur_len > new_len && !self.extents.is_empty() {
            let extent: &mut FileExtent = self.extents.last_mut().unwrap();
            let diff = extent.size().min(cur_len - new_len);
            extent.resize(extent.size() - diff);
            cur_len -= diff;
            if extent.size() == 0 {
                self.extents.pop();
            }
        }
        self.size = new_len;
    }

    pub fn len(&self) -> u64 {
        self.size
    }

    pub fn truncate(&mut self) {
        self.extents.clear();
        self.size = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[tracing_test::traced_test]
    pub fn test_offload_file() -> anyhow::Result<()> {
        let buffer = OwnedBuffer::from_bytes(std::iter::repeat(12u8).take(100).collect::<Vec<_>>());
        let test_data2 = buffer.clone();

        let backing = OffloadBackingStore::new(buffer, None);
        let mut file = OffloadedFile::new(None, backing);

        let mut cursor = 0u64;
        let test_data = std::iter::repeat(56u8).take(100).collect::<Vec<_>>();
        file.write(OffloadWrite::Buffer(&test_data), &mut cursor)?;

        assert_eq!(file.len(), 100);

        cursor = 0;
        let mut result = std::iter::repeat(0u8).take(100).collect::<Vec<_>>();
        file.read(&mut result, &mut cursor)?;
        assert_eq!(
            &result,
            &std::iter::repeat(56u8).take(100).collect::<Vec<_>>()
        );

        cursor = 50;
        file.write(OffloadWrite::Buffer(&test_data2), &mut cursor)?;

        assert_eq!(file.len(), 150);

        cursor = 0;
        let mut result = std::iter::repeat(0u8).take(150).collect::<Vec<_>>();
        file.read(&mut result, &mut cursor)?;
        assert_eq!(
            &result,
            &std::iter::repeat(56u8)
                .take(50)
                .chain(std::iter::repeat(12u8).take(100))
                .collect::<Vec<_>>()
        );

        file.resize(200, 99u8);
        assert_eq!(file.len(), 200);

        cursor = 0;
        let mut result = std::iter::repeat(0u8).take(200).collect::<Vec<_>>();
        file.read(&mut result, &mut cursor)?;
        assert_eq!(
            &result,
            &std::iter::repeat(56u8)
                .take(50)
                .chain(std::iter::repeat(12u8).take(100))
                .chain(std::iter::repeat(99u8).take(50))
                .collect::<Vec<_>>()
        );

        file.resize(33, 1u8);

        cursor = 0;
        let mut result = std::iter::repeat(0u8).take(33).collect::<Vec<_>>();
        file.read(&mut result, &mut cursor)?;
        assert_eq!(
            &result,
            &std::iter::repeat(56u8).take(33).collect::<Vec<_>>()
        );

        let mut cursor = 10u64;
        let test_data = std::iter::repeat(74u8).take(10).collect::<Vec<_>>();
        file.write(OffloadWrite::Buffer(&test_data), &mut cursor)?;

        assert_eq!(file.len(), 33);

        cursor = 0;
        let mut result = std::iter::repeat(0u8).take(33).collect::<Vec<_>>();
        file.read(&mut result, &mut cursor)?;
        assert_eq!(
            &result,
            &std::iter::repeat(56u8)
                .take(10)
                .chain(std::iter::repeat(74u8).take(10))
                .chain(std::iter::repeat(56u8).take(13))
                .collect::<Vec<_>>()
        );

        Ok(())
    }
}
