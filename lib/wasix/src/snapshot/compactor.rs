use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Mutex,
};

use futures::future::{BoxFuture, LocalBoxFuture};
use sha2::{Digest, Sha256};
use virtual_fs::Fd;

use super::*;

struct State {
    memory_map: HashMap<Range<u64>, [u8; 32]>,
    open_file: HashMap<Fd, FdSnapshot<'static>>,
    close_file: HashSet<Fd>,
}

/// Deduplicates memory and stacks to reduce the number of volume of
/// log events sent to its inner capturer. Compacting the events occurs
/// in line as the events are generated
pub struct CompactingSnapshotCapturer {
    inner: Box<DynSnapshotCapturer>,
    state: Mutex<State>,
}

impl CompactingSnapshotCapturer {
    pub fn new(inner: Box<DynSnapshotCapturer>) -> Self {
        Self {
            inner,
            state: Mutex::new(State {
                memory_map: Default::default(),
                open_file: Default::default(),
                close_file: Default::default(),
            }),
        }
    }
}

impl SnapshotCapturer for CompactingSnapshotCapturer {
    fn write<'a>(&'a self, entry: SnapshotLog<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async {
            match entry {
                SnapshotLog::UpdateMemoryRegion { region, data } => {
                    let mut hasher = Sha256::default();
                    hasher.update(data.as_ref());
                    let hash: [u8; 32] = hasher.finalize().try_into().unwrap();

                    {
                        let mut state = self.state.lock().unwrap();
                        if let Some(other) = state.memory_map.get_mut(&region) {
                            if *other == hash {
                                return Ok(());
                            } else {
                                *other = hash;
                            }
                        } else {
                            let to_remove = state
                                .memory_map
                                .keys()
                                .filter(|r| {
                                    // Covers the whole range
                                    (
                                    region.start <= r.start &&
                                    region.end >= r.end
                                ) ||
                                // Clips the left side
                                (
                                    region.start <= r.start &&
                                    region.end > r.start
                                ) ||
                                // Clips the right side
                                (
                                    region.start < r.end &&
                                    region.end >= r.end
                                )
                                })
                                .cloned()
                                .collect::<Vec<_>>();
                            for r in to_remove {
                                state.memory_map.remove(&r);
                            }

                            state.memory_map.insert(region.clone(), hash);
                        }
                    }
                    return self
                        .inner
                        .write(SnapshotLog::UpdateMemoryRegion { region, data })
                        .await;
                }
                SnapshotLog::CloseFileDescriptor { fd } => {
                    let mut state = self.state.lock().unwrap();
                    state.open_file.remove(&fd);
                    state.close_file.insert(fd);
                }
                SnapshotLog::OpenFileDescriptor {
                    fd,
                    state: fd_state,
                } => {
                    let mut state = self.state.lock().unwrap();
                    state.close_file.remove(&fd);
                    state.open_file.insert(fd, fd_state.into_owned());
                }
                SnapshotLog::Snapshot { .. } => {
                    let (to_close, to_open) = {
                        let mut state = self.state.lock().unwrap();
                        (
                            state.close_file.drain().collect::<Vec<_>>(),
                            state.open_file.drain().collect::<Vec<_>>(),
                        )
                    };
                    for fd in to_close {
                        self.inner
                            .write(SnapshotLog::CloseFileDescriptor { fd })
                            .await?;
                    }
                    for (fd, fd_state) in to_open {
                        self.inner
                            .write(SnapshotLog::OpenFileDescriptor {
                                fd,
                                state: fd_state,
                            })
                            .await?;
                    }
                    return self.inner.write(entry).await;
                }
                entry => {
                    return self.inner.write(entry).await;
                }
            }
            Ok(())
        })
    }

    fn read<'a>(&'a self) -> BoxFuture<'a, anyhow::Result<Option<SnapshotLog<'a>>>> {
        Box::pin(async {
            Ok(match self.inner.read().await? {
                Some(SnapshotLog::UpdateMemoryRegion { region, data }) => {
                    let mut hasher = Sha256::default();
                    hasher.update(data.as_ref());
                    let hash: [u8; 32] = hasher.finalize().try_into().unwrap();

                    let mut state = self.state.lock().unwrap();
                    state.memory_map.insert(region.clone(), hash);

                    Some(SnapshotLog::UpdateMemoryRegion { region, data })
                }
                Some(entry) => Some(entry),
                None => None,
            })
        })
    }
}
