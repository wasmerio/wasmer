//! Private byte protocol from include/uapi/linux/fuse.h. These records never cross a
//! typed syscall boundary: libfuse exchanges them through read/write of /dev/fuse.
use wasixv2_syscalls::Errno;

pub(super) const HEADER: usize = 40;
pub(super) const OUT_HEADER: usize = 16;
pub(super) const MIN_READ: usize = 8192;
pub(super) const MAX_IO: usize = 65536;
pub(super) const MINOR: u32 = 46;
pub(super) const INIT: u32 = 26;
pub(super) const INTERRUPT: u32 = 36;
pub(super) const FORGET: u32 = 2;
pub(super) const BATCH_FORGET: u32 = 42;
pub(super) const DESTROY: u32 = 38;

pub(super) fn u32_at(bytes: &[u8], at: usize) -> Result<u32, Errno> {
    let word = bytes.get(at..at + 4).ok_or(Errno::EINVAL)?;
    Ok(u32::from_le_bytes(word.try_into().unwrap()))
}
pub(super) fn u64_at(bytes: &[u8], at: usize) -> Result<u64, Errno> {
    let word = bytes.get(at..at + 8).ok_or(Errno::EINVAL)?;
    Ok(u64::from_le_bytes(word.try_into().unwrap()))
}
pub(super) fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn words32(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|v| v.to_le_bytes()).collect()
}
pub(super) fn words64(words: &[u64]) -> Vec<u8> {
    words.iter().flat_map(|v| v.to_le_bytes()).collect()
}
pub(super) fn name(body: &mut Vec<u8>, name: &[u8]) {
    body.extend_from_slice(name);
    body.push(0);
}
