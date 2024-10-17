use crate::VIRTUAL_ROOT_FD;

use super::*;

impl JournalEffector {
    pub fn save_path_set_times(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        flags: LookupFlags,
        path: String,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    ) -> anyhow::Result<()> {
        Self::save_event(
            ctx,
            JournalEntry::PathSetTimesV1 {
                fd,
                flags,
                path: path.into(),
                st_atim,
                st_mtim,
                fst_flags,
            },
        )
    }

    pub fn apply_path_set_times(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        fd: Fd,
        flags: LookupFlags,
        path: &str,
        st_atim: Timestamp,
        st_mtim: Timestamp,
        fst_flags: Fstflags,
    ) -> anyhow::Result<()> {
        // see `VIRTUAL_ROOT_FD` for details as to why this exists
        if fd == VIRTUAL_ROOT_FD {
            // we ignore this record as its not implemented yet
        } else {
            crate::syscalls::path_filestat_set_times_internal(ctx, fd, flags, path, st_atim, st_mtim, fst_flags)
                .map_err(|err| {
                    anyhow::format_err!(
                        "journal restore error: failed to set path times (fd={}, flags={}, path={}, st_atim={}, st_mtim={}, fst_flags={:?}) - {}",
                        fd,
                        flags,
                        path,
                        st_atim,
                        st_mtim,
                        fst_flags,
                        err
                    )
                })?;
        }
        Ok(())
    }
}
