use crate::pagination::{self, Cursor, Page};
use crate::{authz, App, AppError};
use domain::{Principal, Role};
use infrastructure::audit::{self, AuditEntry, AuditRow};
use infrastructure::repo;

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

pub struct AdminUser {
    pub id: uuid::Uuid,
    pub email: String,
    pub display_name: String,
    pub status: String,
    pub roles: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<repo::AdminUserRow> for AdminUser {
    fn from(r: repo::AdminUserRow) -> Self {
        Self {
            id: r.id,
            email: r.email,
            display_name: r.display_name,
            status: r.status,
            roles: r.roles,
            created_at: r.created_at,
        }
    }
}

/// Admin-only user directory; `q` matches email or display name (case-insensitive substring).
pub async fn list_users(
    app: &App,
    principal: &Principal,
    q: Option<&str>,
    cursor: Option<&str>,
    limit: Option<i64>,
) -> Result<Page<AdminUser>, AppError> {
    authz::require_role(principal, Role::Admin)?;
    let limit = pagination::limit(limit)?;
    let after = cursor.map(Cursor::decode).transpose()?.map(|c| (c.ts, c.id));
    let pattern = q.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
        let esc: String = s
            .chars()
            .take(100)
            .flat_map(|c| match c {
                '%' | '_' | '\\' => vec!['\\', c],
                _ => vec![c],
            })
            .collect();
        format!("%{esc}%")
    });
    let mut conn = app.pool.acquire().await?;
    let rows = repo::list_users(&mut conn, pattern.as_deref(), after, limit + 1).await?;
    Ok(Page::from_overfetch(
        rows.into_iter().map(AdminUser::from).collect(),
        limit,
        |u: &AdminUser| Cursor {
            ts: u.created_at,
            id: u.id,
        },
    ))
}

#[derive(Clone, Copy)]
pub enum Change {
    Grant(Role),
    Revoke(Role),
    Disable,
    Enable,
}

pub fn parse_role(s: &str) -> Result<Role, AppError> {
    Role::parse(s).ok_or_else(|| {
        AppError::Validation(vec![domain::FieldViolation {
            field: "role",
            code: "invalid",
        }])
    })
}

/// Grants or revokes a role, or disables/enables an account. Idempotent. Admins cannot change their own
/// account (so the last admin can never lock themselves out). The row lock serialises concurrent changes;
/// the change and its audit entry commit together. Disabling ends every session of the user.
pub async fn change_user(
    app: &App,
    principal: &Principal,
    meta: &crate::RequestMeta,
    user_id: uuid::Uuid,
    change: Change,
) -> Result<AdminUser, AppError> {
    authz::require_role(principal, Role::Admin)?;
    if user_id == principal.user_id {
        return Err(AppError::Conflict {
            code: "own_account",
            current_revision_id: None,
        });
    }
    let mut tx = app.pool.begin().await?;
    let before = repo::lock_user(&mut tx, user_id)
        .await?
        .ok_or(AppError::NotFound { entity: "user" })?;
    let (action, detail) = match change {
        Change::Grant(r) => (
            if repo::grant_role(&mut tx, user_id, r, Some(principal.user_id)).await? {
                "admin.role_granted"
            } else {
                "admin.role_unchanged"
            },
            Some(r),
        ),
        Change::Revoke(r) => (
            if repo::revoke_role(&mut tx, user_id, r).await? {
                "admin.role_revoked"
            } else {
                "admin.role_unchanged"
            },
            Some(r),
        ),
        Change::Disable | Change::Enable => {
            let disable = matches!(change, Change::Disable);
            let after = repo::set_disabled(&mut tx, user_id, disable).await?;
            if disable {
                repo::delete_user_sessions(&mut tx, user_id).await?;
            }
            (
                if after == before {
                    "admin.status_unchanged"
                } else if disable {
                    "admin.user_disabled"
                } else {
                    "admin.user_enabled"
                },
                None,
            )
        }
    };
    if !action.ends_with("unchanged") {
        audit::record(
            &mut tx,
            AuditEntry {
                actor_id: Some(principal.user_id),
                request_id: meta.request_id.as_deref(),
                before: Some(serde_json::json!({ "status": before })),
                metadata: serde_json::json!({ "role": detail.map(Role::as_str) }),
                ..AuditEntry::new(action, "user", Some(user_id))
            },
        )
        .await?;
    }
    let user = one(&mut tx, user_id).await?;
    tx.commit().await?;
    Ok(user)
}

async fn one(conn: &mut sqlx::PgConnection, id: uuid::Uuid) -> Result<AdminUser, AppError> {
    repo::get_admin_user(conn, id)
        .await?
        .map(AdminUser::from)
        .ok_or(AppError::NotFound { entity: "user" })
}
