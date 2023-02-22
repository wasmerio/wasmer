use wcgi_host::CgiDialect;

// FIXME(@Michael-F-Bryan): Make this public in the webc crate
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WasiCommandAnnotation {
    #[serde(default)]
    pub atom: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub env: Option<Vec<String>>,
    #[serde(default)]
    pub main_args: Option<Vec<String>>,
    #[serde(default, rename = "mountAtomInVolume")]
    pub mount_atom_in_volume: Option<String>,
}

// FIXME(@Michael-F-Bryan): Add this to the webc crate and update
// wapm-targz-to-pirita
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WcgiAnnotation {
    #[serde(default)]
    pub dialect: Option<CgiDialect>,
}
