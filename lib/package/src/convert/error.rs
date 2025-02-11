use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ConversionError {
    message: String,
    cause: Option<Arc<dyn std::error::Error + Send + Sync>>,
}

impl ConversionError {
    pub fn msg(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            cause: None,
        }
    }

    pub fn with_cause(
        msg: impl Into<String>,
        cause: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            message: msg.into(),
            cause: Some(Arc::new(cause)),
        }
    }
}

impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not convert manifest: {}", self.message)?;
        if let Some(cause) = &self.cause {
            write!(f, " (cause: {cause})")?;
        }

        Ok(())
    }
}

impl std::error::Error for ConversionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Some(e) = &self.cause {
            Some(e)
        } else {
            None
        }
    }
}
