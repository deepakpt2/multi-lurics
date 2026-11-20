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

/// True for the record's creator and for moderators. Everyone else signed in may only fill in what is blank.
pub fn is_editor(p: &Principal, created_by: uuid::Uuid) -> bool {
    p.user_id == created_by || p.has_role(Role::Moderator)
}

/// Catalogue records may be edited by their creator or by a moderator (admin implies moderator).
pub fn require_editor(p: &Principal, created_by: uuid::Uuid) -> Result<(), AppError> {
    if p.user_id == created_by || p.has_role(Role::Moderator) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
