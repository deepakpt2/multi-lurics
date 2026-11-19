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

/// Catalogue records may be edited by their creator or by a moderator (admin implies moderator).
pub fn require_editor(p: &Principal, created_by: uuid::Uuid) -> Result<(), AppError> {
    if p.user_id == created_by || p.has_role(Role::Moderator) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
