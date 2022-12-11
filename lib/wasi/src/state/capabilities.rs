use crate::http::HttpClientCapabilityV1;

#[derive(Clone, Debug)]
pub struct Capabilities {
    pub insecure_allow_all: bool,
    pub http: HttpClientCapabilityV1,
}

impl Capabilities {
    pub fn new() -> Self {
        Self {
            insecure_allow_all: false,
            http: Default::default(),
        }
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::new()
    }
}
