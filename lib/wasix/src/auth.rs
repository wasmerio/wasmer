use std::{fmt::Debug, ops::Deref};

use anyhow::Error;

/// Authentication with the Wasmer registry.
pub trait Authentication: Debug {
    /// Look up a token that can be used when authenticating with the provided
    /// registry.
    ///
    /// Note that the `url` isn't guaranteed to point exactly at a registry's
    /// GraphQL endpoint, so some massaging may be required.
    fn get_token(&self, url: &str) -> Result<Option<String>, Error>;
}

impl<P, A> Authentication for P
where
    P: Deref<Target = A> + Debug,
    A: Authentication,
{
    fn get_token(&self, url: &str) -> Result<Option<String>, Error> {
        (**self).get_token(url)
    }
}
