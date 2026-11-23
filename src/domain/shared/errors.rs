//! Domain error primitives.

use thiserror::Error;

/// One invalid input field and why it was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// Errors raised by domain rules. Validation reports every invalid field at
/// once so clients can fix a payload in a single attempt.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    #[error("validation failed for {} field(s)", .0.len())]
    Validation(Vec<FieldError>),
    /// An update named no updatable field.
    #[error("at least one updatable field must be provided")]
    NoChanges,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_counts_fields() {
        let err = DomainError::Validation(vec![
            FieldError::new("title", "must be between 1 and 200 characters"),
            FieldError::new("priority", "must be one of: low, medium, high"),
        ]);
        assert_eq!(err.to_string(), "validation failed for 2 field(s)");
    }

    #[test]
    fn no_changes_has_a_message() {
        assert_eq!(
            DomainError::NoChanges.to_string(),
            "at least one updatable field must be provided"
        );
    }
}
