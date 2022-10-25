pub static ALL_WASI_VERSIONS: &[WasiVersion] = &[WasiVersion::Unstable, WasiVersion::Snapshot1];
pub static LATEST_WASI_VERSION: &[WasiVersion] = &[WasiVersion::get_latest()];
pub static NIGHTLY_VERSION: &[WasiVersion] = &[WasiVersion::current_nightly()];

#[derive(Debug, Clone, Copy)]
pub enum WasiVersion {
    /// A.K.A. Snapshot0
    Unstable,
    Snapshot1,
    /// This is for making tests pass on Apple M1 while
    /// still keeping the old test for compatibility reasons
    #[allow(non_camel_case_types)]
    Nightly_2022_10_18,
}

impl WasiVersion {
    pub const fn get_latest() -> Self {
        Self::Snapshot1
    }

    pub const fn current_nightly() -> Self {
        Self::Nightly_2022_10_18
    }

    pub fn get_compiler_toolchain(&self) -> &'static str {
        match self {
            WasiVersion::Unstable => "nightly-2019-09-13",
            WasiVersion::Snapshot1 => "1.53.0",
            WasiVersion::Nightly_2022_10_18 => "nightly-2022-10-18",
        }
    }

    pub fn get_directory_name(&self) -> &'static str {
        match self {
            WasiVersion::Unstable => "unstable",
            WasiVersion::Snapshot1 => "snapshot1",
            WasiVersion::Nightly_2022_10_18 => "nightly_2022_10_18",
        }
    }
}
