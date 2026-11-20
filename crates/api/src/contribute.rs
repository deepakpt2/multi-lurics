//! One form for a whole song, open to every signed-in member.

use crate::extract::{ApiJson, Meta, RequireUser};
use crate::problem::{ApiResult, Problem};
use application::contribute as uc;
use application::App;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// A person or album that may already exist: the `id` of one chosen from suggestions, or just a `name`.
#[derive(Deserialize, ToSchema, Default, Clone)]
pub struct NameRef {
    pub id: Option<Uuid>,
    #[serde(default)]
    pub name: String,
}

impl From<NameRef> for uc::NameRef {
    fn from(r: NameRef) -> Self {
        Self { id: r.id, name: r.name }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct LyricBlock {
    /// `original`, `translation` or `transliteration`. A translation or transliteration needs no original.
    #[serde(rename = "type")]
    pub kind: String,
    /// `ml`, `hi`, `mal`… or the English name.
    pub language: String,
    /// The language's usual script when blank.
    #[serde(default)]
    pub script: String,
    pub body: String,
}

#[derive(Deserialize, ToSchema)]
pub struct SongSubmission {
    pub title: String,
    pub year: Option<i32>,
    pub album: Option<NameRef>,
    #[serde(default)]
    pub singers: Vec<NameRef>,
    #[serde(default)]
    pub composers: Vec<NameRef>,
    #[serde(default)]
    pub lyricists: Vec<NameRef>,
    /// `mainstream` (default) or `devotional`.
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub religion: String,
    #[serde(default)]
    pub deity: String,
    #[serde(default)]
    pub raga: String,
    #[serde(default)]
    pub devotional_form: String,
    #[serde(default)]
    pub source_work: String,
    #[serde(default)]
    pub occasions: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// A YouTube link in any form.
    #[serde(default)]
    pub youtube: String,
    /// At least one block with text.
    pub lyrics: Vec<LyricBlock>,
    /// Make the song even if one with this title is already on the album (otherwise 409 `possible_duplicate`).
    #[serde(default)]
    pub allow_duplicate: bool,
}

#[derive(Serialize, ToSchema)]
pub struct SongMade {
    pub composition_id: Uuid,
    pub recording_id: Uuid,
    /// `published` or `pending` (waiting for review), one per lyric block with text, in order.
    pub statuses: Vec<String>,
}

#[utoipa::path(post, path = "/api/v1/contribute/song", operation_id = "contribute_song", tag = "contribute",
    request_body = SongSubmission, security(("session" = [])), responses(
    (status = 201, body = SongMade),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 409, body = Problem, content_type = "application/problem+json", description = "`possible_duplicate`: a song with this title is already on the album; `candidates` lists them."),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn contribute_song(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    ApiJson(b): ApiJson<SongSubmission>,
) -> ApiResult<(axum::http::StatusCode, Json<SongMade>)> {
    let refs = |v: Vec<NameRef>| v.into_iter().map(Into::into).collect();
    let made = uc::song(
        &app,
        &p,
        &meta,
        uc::Submission {
            title: b.title,
            year: b.year,
            album: b.album.map(Into::into),
            singers: refs(b.singers),
            composers: refs(b.composers),
            lyricists: refs(b.lyricists),
            category: b.category,
            religion: b.religion,
            deity: b.deity,
            raga: b.raga,
            devotional_form: b.devotional_form,
            source_work: b.source_work,
            occasions: b.occasions,
            tags: b.tags,
            youtube: b.youtube,
            lyrics: b
                .lyrics
                .into_iter()
                .map(|l| uc::LyricBlock {
                    kind: l.kind,
                    language: l.language,
                    script: l.script,
                    body: l.body,
                })
                .collect(),
            allow_duplicate: b.allow_duplicate,
        },
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(SongMade {
            composition_id: made.composition_id,
            recording_id: made.recording_id,
            statuses: made.statuses,
        }),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct LyricAdded {
    /// `published` or `pending` (waiting for review).
    pub status: String,
}

#[utoipa::path(post, path = "/api/v1/compositions/{id}/lyrics", operation_id = "add_song_lyrics", tag = "contribute",
    request_body = LyricBlock, params(("id" = Uuid, Path)), security(("session" = [])), responses(
    (status = 201, body = LyricAdded),
    (status = 401, body = Problem, content_type = "application/problem+json"),
    (status = 404, body = Problem, content_type = "application/problem+json"),
    (status = 422, body = Problem, content_type = "application/problem+json")))]
pub async fn add_song_lyrics(
    State(app): State<App>,
    Meta(meta): Meta,
    RequireUser(p): RequireUser,
    crate::extract::ApiPath(id): crate::extract::ApiPath<Uuid>,
    ApiJson(b): ApiJson<LyricBlock>,
) -> ApiResult<(axum::http::StatusCode, Json<LyricAdded>)> {
    let status = uc::lyric_for_song(
        &app,
        &p,
        &meta,
        id,
        uc::LyricBlock {
            kind: b.kind,
            language: b.language,
            script: b.script,
            body: b.body,
        },
    )
    .await?;
    Ok((axum::http::StatusCode::CREATED, Json(LyricAdded { status })))
}
