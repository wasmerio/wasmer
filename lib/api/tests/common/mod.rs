/// An exit, as WASIX reports one: an error of the embedder's own type.
#[derive(Debug)]
pub struct Exit(pub i32);

impl std::fmt::Display for Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "exited with {}", self.0)
    }
}

impl std::error::Error for Exit {}
