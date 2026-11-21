//! Application error: the only error type that crosses from use cases to the
//! HTTP layer.

use thiserror::Error;

use crate::domain::shared::{DomainError, FieldError};

type Source = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, Error)]
pub enum AppError {
    /// One or more input fields are invalid.
    #[error("validation failed")]
    Validation(Vec<FieldError>),
    /// The requested resource does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// A dependency (the database) is temporarily unavailable.
    #[error("service unavailable: {0}")]
    Unavailable(#[source] Source),
    /// Anything unexpected. The cause is logged, never returned to clients.
    #[error("internal error: {0}")]
    Internal(#[source] Source),
}

impl From<DomainError> for AppError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::Validation(fields) => AppError::Validation(fields),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_validation_converts_to_app_validation() {
        let err: AppError =
            DomainError::Validation(vec![FieldError::new("title", "required")]).into();
        match err {
            AppError::Validation(fields) => assert_eq!(fields.len(), 1),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
