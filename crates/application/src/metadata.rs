//! Admins can create metadata values (people, albums, ragas, deities, tags) ahead of the people who will
//! use them, so contributors find them in the suggestions instead of making a second copy. A name that
//! already exists (by name, alias or spelling in any script) is reported, not duplicated.

use crate::catalogue::{self as cat, AlbumInput, ArtistInput};
use crate::{authz, App, AppError, RequestMeta};
use domain::{Principal, Role};
use infrastructure::catalogue as repo;
use infrastructure::AppTx;
use uuid::Uuid;

pub const MAX_NAMES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Artist,
    Album,
    Raga,
    Deity,
    Tag,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "artist" => Self::Artist,
            "album" => Self::Album,
            "raga" => Self::Raga,
            "deity" => Self::Deity,
            "tag" => Self::Tag,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Created {
    pub name: String,
    pub id: Option<Uuid>,
    /// True when it was made now; false when it was already there.
    pub created: bool,
    /// A problem with this name (too long, blank…), in place of a result.
    pub problem: Option<String>,
}

pub async fn create(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    kind: Kind,
    names: Vec<String>,
    sub_kind: Option<String>,
    year: Option<i32>,
) -> Result<Vec<Created>, AppError> {
    authz::require_role(p, Role::Admin)?;
    let names: Vec<String> = names
        .into_iter()
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .collect();
    if names.is_empty() {
        return Err(domain::DomainError::invalid("names", "required").into());
    }
    if names.len() > MAX_NAMES {
        return Err(domain::DomainError::invalid("names", "too_many").into());
    }
    let mut out = Vec::new();
    for name in names {
        let r = one(app, p, meta, kind, &name, sub_kind.clone(), year).await;
        out.push(match r {
            Ok((id, created)) => Created {
                name,
                id: Some(id),
                created,
                problem: None,
            },
            Err(AppError::Validation(v)) => Created {
                name,
                id: None,
                created: false,
                problem: v.first().map(|f| f.code.to_owned()),
            },
            Err(e) => return Err(e),
        });
    }
    Ok(out)
}

async fn one(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    kind: Kind,
    name: &str,
    sub_kind: Option<String>,
    year: Option<i32>,
) -> Result<(Uuid, bool), AppError> {
    match kind {
        Kind::Artist => {
            let found: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM (
                   SELECT id, 0 AS rank, created_at FROM artists WHERE lower(name) = lower($1)
                   UNION ALL
                   SELECT entity_id, 1, now() FROM search_documents
                    WHERE entity_type = 'artist' AND compact = replace(lyrics_search_key($1), ' ', '')
                 ) x ORDER BY rank, created_at LIMIT 1",
            )
            .bind(name)
            .fetch_optional(&app.pool)
            .await?;
            if let Some(id) = found {
                return Ok((id, false));
            }
            let a = cat::create_artist(
                app,
                p,
                meta,
                ArtistInput {
                    name: Some(name.to_owned()),
                    kind: Some(sub_kind.unwrap_or_else(|| "person".into())),
                    ..Default::default()
                },
            )
            .await?;
            Ok((a.artist.id, true))
        }
        Kind::Album => {
            let found: Option<Uuid> =
                sqlx::query_scalar("SELECT id FROM albums WHERE lower(title) = lower($1) ORDER BY created_at LIMIT 1")
                    .bind(name)
                    .fetch_optional(&app.pool)
                    .await?;
            if let Some(id) = found {
                return Ok((id, false));
            }
            let a = cat::create_album(
                app,
                p,
                meta,
                AlbumInput {
                    title: Some(name.to_owned()),
                    kind: Some(sub_kind.unwrap_or_else(|| "album".into())),
                    release_year: Some(year),
                    ..Default::default()
                },
            )
            .await?;
            Ok((a.album.id, true))
        }
        Kind::Raga | Kind::Deity | Kind::Tag => {
            let (k, table): (&'static str, &str) = match kind {
                Kind::Raga => ("raga", "ragas"),
                Kind::Deity => ("deity", "deities"),
                _ => ("tag", "tags"),
            };
            if name.chars().count() > 100 {
                return Err(domain::DomainError::invalid("names", "too_long").into());
            }
            let mut tx = AppTx::begin(&app.pool).await?;
            let before: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&mut *tx)
                .await?;
            let (id, _) = repo::resolve_term(&mut tx, k, name, p.user_id).await?;
            let after: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok((id, after > before))
        }
    }
}
