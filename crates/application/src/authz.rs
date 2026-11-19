use crate::AppError;
use domain::{Principal, Role};

/// Authorization is decided here, from the server-side principal only.
pub fn require_role(p: &Principal, role: Role) -> Result<(), AppError> {
    if p.has_role(role) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
