use domain::{DomainError, FieldViolation};
use uuid::Uuid;

/// Every failure a use case can report. The API layer maps each to RFC 7807.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("validation failed")]
    Validation(Vec<FieldViolation>),
    #[error("malformed request: {0}")]
    BadRequest(&'static str),
    #[error("authentication required")]
    Unauthenticated,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("email address not verified")]
    EmailNotVerified,
    #[error("forbidden")]
    Forbidden,
    #[error("token invalid or expired")]
    InvalidToken,
    #[error("{entity} not found")]
    NotFound { entity: &'static str },
    /// Optimistic-concurrency failure. `current_revision_id` lets the client rebase.
    #[error("conflict: {code}")]
    Conflict {
        code: &'static str,
        current_revision_id: Option<Uuid>,
    },
    #[error("service unavailable")]
    Unavailable,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<DomainError> for AppError {
    fn from(e: DomainError) -> Self {
        match e {
            DomainError::Invalid(v) => Self::Validation(v),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::PoolTimedOut => Self::Unavailable,
            other => Self::Internal(other.into()),
        }
    }
}
