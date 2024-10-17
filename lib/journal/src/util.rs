use super::*;

pub fn copy_journal<R: ReadableJournal, W: WritableJournal>(
    from: &R,
    to: &W,
) -> anyhow::Result<()> {
    while let Some(record) = from.read()? {
        to.write(record.into_inner())?;
    }
    Ok(())
}
