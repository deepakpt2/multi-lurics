use crate::pagination::{self, Cursor, Page};
use crate::{authz, App, AppError};
use domain::{Principal, Role};
use infrastructure::audit::{self, AuditRow};

/// Admin-only, newest-first audit trail.
pub async fn list_audit(
    app: &App,
    principal: &Principal,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<AuditRow>, AppError> {
    // Checked here as well as by the route extractor: use cases never rely on the caller.
    authz::require_role(principal, Role::Admin)?;
    let limit = pagination::limit(limit)?;
    let after = cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id));
    let mut conn = app.pool.acquire().await?;
    let rows = audit::list(&mut conn, after, limit + 1).await?;
    Ok(Page::from_overfetch(rows, limit, |r| Cursor {
        ts: r.occurred_at,
        id: r.id,
    }))
}
