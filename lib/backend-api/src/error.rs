///  One or multiple errors returned by the GraphQL API.
// Mainly exists to implement [`std::error::Error`].
#[derive(Debug)]
pub struct GraphQLApiFailure {
    pub errors: Vec<cynic::GraphQlError>,
}

impl GraphQLApiFailure {
    pub fn from_errors(
        msg: impl Into<String>,
        errors: Option<Vec<cynic::GraphQlError>>,
    ) -> anyhow::Error {
        let msg = msg.into();
        if let Some(errs) = errors {
            if !errs.is_empty() {
                let err = GraphQLApiFailure { errors: errs };
                return anyhow::Error::new(err).context(msg);
            }
        }
        anyhow::anyhow!("{msg} - query did not return any data")
    }
}

impl std::fmt::Display for GraphQLApiFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let errs = self
            .errors
            .iter()
            .map(|err| err.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "GraphQL API failure: {errs}")
    }
}

impl std::error::Error for GraphQLApiFailure {}
