//! Pure domain model: value objects, entities and invariants. No I/O.

pub mod identity;

pub use identity::{DisplayName, Email, Password, Principal, Role, UserStatus};

/// A single invalid field. `code` is a stable, machine-readable key that the
/// frontend maps to a localized message; never a human sentence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FieldViolation {
    pub field: &'static str,
    pub code: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("validation failed")]
    Invalid(Vec<FieldViolation>),
}

impl DomainError {
    pub fn invalid(field: &'static str, code: &'static str) -> Self {
        Self::Invalid(vec![FieldViolation { field, code }])
    }
}

/// Collects violations across several fields so a client sees all of them at once.
#[derive(Debug, Default)]
pub struct Violations(Vec<FieldViolation>);

impl Violations {
    pub fn check<T>(&mut self, result: Result<T, DomainError>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(DomainError::Invalid(v)) => {
                self.0.extend(v);
                None
            }
        }
    }

    pub fn push(&mut self, field: &'static str, code: &'static str) {
        self.0.push(FieldViolation { field, code });
    }

    pub fn finish(self) -> Result<(), DomainError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(DomainError::Invalid(self.0))
        }
    }
}
