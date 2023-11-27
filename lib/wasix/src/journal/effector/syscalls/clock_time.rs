use super::*;

impl JournalEffector {
    pub fn save_clock_time_set(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        clock_id: Snapshot0Clockid,
        time: Timestamp,
    ) -> anyhow::Result<()> {
        Self::save_event(ctx, JournalEntry::SetClockTimeV1 { clock_id, time })
    }

    pub fn apply_clock_time_set(
        ctx: &mut FunctionEnvMut<'_, WasiEnv>,
        clock_id: Snapshot0Clockid,
        time: Timestamp,
    ) -> anyhow::Result<()> {
        let ret = crate::syscalls::clock_time_set_internal(ctx, clock_id, time);
        if ret != Errno::Success {
            bail!(
                "journal restore error: failed to set clock time (clock_id={:?}, time={}) - {}",
                clock_id,
                time,
                ret
            );
        }
        Ok(())
    }
}
