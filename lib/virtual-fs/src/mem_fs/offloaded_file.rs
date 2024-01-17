use bytes::Bytes;
use shared_buffer::OwnedBuffer;
use std::{cmp, io};

use crate::limiter::DynFsMemoryLimiter;

#[derive(Debug)]
pub enum FileExtent {
    MmapOffload { offset: u64, size: u64 },
    RepeatingBytes { value: u8, cnt: u64 },
    Bytes { data: Bytes },
}

impl FileExtent {
    pub fn size(&self) -> u64 {
        match self {
            FileExtent::MmapOffload { size, .. } => *size,
            FileExtent::RepeatingBytes { cnt, .. } => *cnt,
            FileExtent::Bytes { data } => data.len() as u64,
        }
    }

    pub fn resize(&mut self, new_size: u64) {
        match self {
            FileExtent::MmapOffload { size, .. } => *size = new_size.min(*size),
            FileExtent::RepeatingBytes { cnt, .. } => *cnt = new_size,
            FileExtent::Bytes { data } => {
                *data = data.slice(..(new_size as usize));
            }
        }
    }
}

#[derive(Debug)]
pub struct OffloadedFile {
    mmap_offload: OwnedBuffer,
    #[allow(dead_code)]
    limiter: Option<DynFsMemoryLimiter>,
    extents: Vec<FileExtent>,
}

impl OffloadedFile {
    pub fn new(limiter: Option<DynFsMemoryLimiter>, buffer: OwnedBuffer) -> Self {
        Self {
            mmap_offload: buffer,
            limiter,
            extents: Vec::new(),
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
        *cursor = cmp::min(self.len() as u64, next_cursor);
        Ok(*cursor)
    }

    pub fn read(&self, mut buf: &mut [u8], cursor: &mut u64) -> io::Result<usize> {
        let cursor_start = *cursor;

        let mut extent_offset = cursor_start;
        let mut extent_index = 0usize;
        while buf.len() > 0 && extent_index < self.extents.len() {
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
                    let mmap_offset = mmap_offset + extent_offset;
                    let data = &self.mmap_offload.as_slice()[mmap_offset as usize..];
                    let data = &data[..(*extent_size - extent_offset) as usize];
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
                FileExtent::Bytes { data } => {
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

    pub fn write(&mut self, data: &[u8], cursor: &mut u64) -> io::Result<usize> {
        let mut extent_offset = *cursor;
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
                        FileExtent::Bytes { data: other_data } => FileExtent::Bytes {
                            data: other_data.slice((split_at as usize)..),
                        },
                    };
                    extent.resize(split_at);
                    self.extents.insert(index + 1, new_extent);
                    break;
                }
            }
        };
        split_extents(extent_offset);
        split_extents(extent_offset + data_len);

        // Now we delete all the extents that exist between the
        // range that we are about to insert
        let mut index = 0usize;
        while extent_offset > 0 && index < self.extents.len() {
            let extent = &self.extents[index];
            if extent_offset > extent.size() {
                extent_offset -= extent.size();
                index += 1;
                continue;
            } else {
                while index < self.extents.len() {
                    let extent = &self.extents[index];
                    if data_len < extent.size() {
                        break;
                    }
                    data_len -= extent.size();
                    self.extents.remove(index);
                }
                break;
            }
        }

        // Finally we add the new extent
        let data_start = data.as_ptr() as u64;
        let data_end = data_start + data.len() as u64;
        let mmap_start = self.mmap_offload.as_slice().as_ptr() as u64;
        let mmap_end = mmap_start + self.mmap_offload.as_slice().len() as u64;

        // If the data is within the mmap buffer then we use a extent range
        // to represent the data, otherwise we fall back on copying the data
        let new_extent = if data_start >= mmap_start && data_end <= mmap_end {
            FileExtent::MmapOffload {
                offset: data_start - mmap_start,
                size: data_end - data_start,
            }
        } else {
            FileExtent::Bytes {
                data: data.to_vec().into(),
            }
        };
        self.extents.insert(index, new_extent);

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
            if extent.size() <= 0 {
                self.extents.pop();
            }
        }
    }

    pub fn len(&self) -> u64 {
        self.extents.iter().map(FileExtent::size).sum()
    }

    pub fn truncate(&mut self) {
        self.extents.clear();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    pub fn test_resize() {}
}
