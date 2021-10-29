pub static ALL_WASI_VERSIONS: &[WasiVersion] = &[WasiVersion::Unstable, WasiVersion::Snapshot1];
pub static LATEST_WASI_VERSION: &[WasiVersion] = &[WasiVersion::get_latest()];

#[derive(Debug, Clone, Copy)]
pub enum WasiVersion {
    /// A.K.A. Snapshot0
    Unstable,
    Snapshot1,
}

impl WasiVersion {
    pub const fn get_latest() -> Self {
        Self::Snapshot1
    }

    pub fn get_compiler_toolchain(&self) -> &'static str {
        match self {
            WasiVersion::Unstable => "nightly-2019-09-13",
            WasiVersion::Snapshot1 => "1.53.0",
        }
    }

    pub fn get_directory_name(&self) -> &'static str {
        match self {
            WasiVersion::Unstable => "unstable",
            WasiVersion::Snapshot1 => "snapshot1",
        }
    }
}
