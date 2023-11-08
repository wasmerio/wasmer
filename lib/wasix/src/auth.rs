use std::{fmt::Debug, ops::Deref};

use anyhow::Error;
use http::HeaderValue;

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

pub(crate) fn header(
    auth: &(impl Authentication + ?Sized),
    url: &str,
) -> Result<Option<HeaderValue>, Error> {
    match auth.get_token(url)? {
        Some(token) => {
            let raw_header = format!("bearer {token}");
            let header = raw_header.parse()?;
            Ok(Some(header))
        }
        None => Ok(None),
    }
}
