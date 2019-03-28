pub struct WasiState<'a> {
    // vfs: Vfs,
    pub args: &'a [Vec<u8>],
    pub envs: &'a [Vec<u8>],
}
