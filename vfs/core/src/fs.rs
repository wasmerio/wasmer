use crate::provider::FsProviderCapabilities;

pub trait Fs: Send + Sync + 'static {
    fn provider_name(&self) -> &'static str;
    fn capabilities(&self) -> FsProviderCapabilities;
}
