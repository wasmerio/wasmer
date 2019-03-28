pub struct WasiState<'a> {
    // vfs: Vfs,
    pub args: &'a [u8],
    pub envs: &'a [u8],
}
