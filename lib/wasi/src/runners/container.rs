use webc::{v1::WebCOwned, v2::read::OwnedReader};

pub struct WebcContainer {
    inner: Container,
}

enum Container {
    V1(WebCOwned),
    V2(OwnedReader),
}
