//! Bulk import of songs: one row per song, validated first (a dry run changes nothing) and then
//! written through the same use cases as the forms, so every rule and audit entry is the same.
//!
//! A row makes, in order: the people it names (found by name or alias, otherwise created), the song with
//! its composer and lyricist credits, a recording with the singers as performers, the album (found by
//! title, otherwise created) with the recording as its next track, and the original lyrics, published
//! straight away because only moderators import. A song that is already there (same title on the same
//! album; without an album, same title and year) is skipped, so running a file twice is safe.

use crate::catalogue::{self as cat, AlbumInput, ArtistInput, CompositionInput, CreditInput, RecordingInput};
use crate::lyrics::{self, ContributionInput};
use crate::{authz, App, AppError, RequestMeta};
use domain::{Principal, Role};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Rows per request. The web app sends a big file in several requests.
pub const MAX_ROWS: usize = 100;

#[derive(Debug, Clone, Default)]
pub struct Row {
    pub title: String,
    /// Language of the lyrics: `ml`, `mal`, or its English name.
    pub language: String,
    /// Script of the lyrics; the language's usual script when blank.
    pub script: String,
    pub lyrics: String,
    pub album: String,
    /// `album` (default), `soundtrack` (a film), `single`, `ep`, `compilation`.
    pub album_kind: String,
    pub year: Option<i32>,
    pub singers: Vec<String>,
    pub composers: Vec<String>,
    pub lyricists: Vec<String>,
    pub category: String,
    pub religion: String,
    pub deity: String,
    pub raga: String,
    pub devotional_form: String,
    pub source_work: String,
    pub occasions: Vec<String>,
    pub tags: Vec<String>,
    pub youtube: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Dry run: this row would be imported.
    WouldCreate,
    Created,
    /// Already in the catalogue; nothing was written.
    Skipped,
    Failed,
}

#[derive(Debug, Clone)]
pub struct RowResult {
    /// 1-based position in the request.
    pub row: usize,
    pub title: String,
    pub status: Status,
    /// For a skipped row, why; for a failed one, the first problem in words a person can act on.
    pub reason: Option<String>,
    pub problems: Vec<(String, String)>,
    pub composition_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub dry_run: bool,
    pub results: Vec<RowResult>,
}

fn blank(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

fn input(r: &Row) -> CompositionInput {
    let devotional = r.category.trim() == "devotional";
    CompositionInput {
        title: Some(r.title.clone()),
        year: Some(r.year),
        category: blank(&r.category),
        raga: Some(blank(&r.raga)),
        religion: devotional.then(|| blank(&r.religion)),
        deity: devotional.then(|| blank(&r.deity)),
        devotional_form: devotional.then(|| blank(&r.devotional_form)),
        source_work: devotional.then(|| blank(&r.source_work)),
        occasions: devotional.then(|| r.occasions.clone()),
        tags: Some(r.tags.clone()),
        youtube: Some(blank(&r.youtube)),
        ..Default::default()
    }
}

/// Everything wrong with a row that can be known without writing: the song's fields, the lyrics, the
/// language. Returns (field, code) pairs.
async fn check(app: &App, r: &Row) -> Result<Vec<(String, String)>, AppError> {
    let mut out: Vec<(String, String)> = Vec::new();
    match cat::validate_composition(&input(r)) {
        Ok(()) => {}
        Err(AppError::Validation(v)) => out.extend(v.into_iter().map(|f| (f.field.to_owned(), f.code.to_owned()))),
        Err(e) => return Err(e),
    }
    if !matches!(
        r.album_kind.trim(),
        "" | "album" | "single" | "ep" | "compilation" | "soundtrack" | "other"
    ) {
        out.push(("album_kind".into(), "invalid_value".into()));
    }
    if !r.lyrics.trim().is_empty() {
        if let Err(domain::DomainError::Invalid(v)) = domain::lyrics::body(&r.lyrics) {
            out.extend(v.into_iter().map(|f| ("lyrics".to_owned(), f.code.to_owned())));
        }
        match language_and_script(app, r).await? {
            Some(_) => {}
            None => out.push((
                "language".into(),
                if r.language.trim().is_empty() {
                    "required"
                } else {
                    "unknown_language"
                }
                .into(),
            )),
        }
    }
    for (field, names) in [
        ("singers", &r.singers),
        ("composers", &r.composers),
        ("lyricists", &r.lyricists),
    ] {
        if names.iter().any(|n| n.trim().chars().count() > 300) {
            out.push((field.into(), "too_long".into()));
        }
    }
    Ok(out)
}

/// The language code to hand to the lyrics use case, and the script (the given one or the language's usual).
pub(crate) async fn language_and_script_of(
    app: &App,
    language: &str,
    script: &str,
) -> Result<Option<(String, String)>, AppError> {
    let lang = language.trim();
    if lang.is_empty() {
        return Ok(None);
    }
    let found: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT coalesce(l.iso639_1, l.iso639_3), s.iso15924
         FROM languages l LEFT JOIN scripts s ON s.id = l.default_script_id
         WHERE lower(l.iso639_1) = lower($1) OR lower(l.iso639_3) = lower($1) OR lower(l.name_en) = lower($1)
         ORDER BY (lower(l.iso639_1) = lower($1)) DESC LIMIT 1",
    )
    .bind(lang)
    .fetch_optional(&app.pool)
    .await?;
    let Some((code, usual)) = found else { return Ok(None) };
    let script = blank(script).or(usual);
    Ok(script.map(|s| (code, s)))
}

async fn language_and_script(app: &App, r: &Row) -> Result<Option<(String, String)>, AppError> {
    language_and_script_of(app, &r.language, &r.script).await
}

async fn duplicate(app: &App, r: &Row, seen: &HashSet<(String, String)>) -> Result<bool, AppError> {
    let title = r.title.trim().to_lowercase();
    let album = r.album.trim().to_lowercase();
    if seen.contains(&(title.clone(), album.clone())) {
        return Ok(true);
    }
    let exists: bool = if album.is_empty() {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM compositions c WHERE lower(c.title) = $1 AND c.year IS NOT DISTINCT FROM $2::smallint)",
        )
        .bind(&title)
        .bind(r.year.map(|y| y as i16))
        .fetch_one(&app.pool)
        .await?
    } else {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM compositions c
               JOIN recordings rc ON rc.composition_id = c.id
               JOIN album_tracks t ON t.recording_id = rc.id
               JOIN albums a ON a.id = t.album_id
             WHERE lower(c.title) = $1 AND lower(a.title) = $2)",
        )
        .bind(&title)
        .bind(&album)
        .fetch_one(&app.pool)
        .await?
    };
    Ok(exists)
}

/// People are found by name first (exact, then any name or alias the search index knows), so "K. J.
/// Yesudas" and "Yesudas" are one person once an alias says so; otherwise created. `cache` keeps the
/// ones met earlier in the same request.
pub(crate) async fn person(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    name: &str,
    cache: &mut HashMap<String, Uuid>,
) -> Result<Uuid, AppError> {
    let key = name.trim().to_lowercase();
    if let Some(id) = cache.get(&key) {
        return Ok(*id);
    }
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM (
           SELECT id, 0 AS rank, created_at FROM artists WHERE lower(name) = $1
           UNION ALL
           SELECT entity_id, 1, now() FROM search_documents
            WHERE entity_type = 'artist' AND compact = replace(lyrics_search_key($2), ' ', '')
         ) x ORDER BY rank, created_at LIMIT 1",
    )
    .bind(&key)
    .bind(name.trim())
    .fetch_optional(&app.pool)
    .await?;
    let id = match found {
        Some(id) => id,
        None => {
            cat::create_artist(
                app,
                p,
                meta,
                ArtistInput {
                    name: Some(name.trim().to_owned()),
                    kind: Some("person".into()),
                    ..Default::default()
                },
            )
            .await?
            .artist
            .id
        }
    };
    cache.insert(key, id);
    Ok(id)
}

async fn people(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    names: &[String],
    cache: &mut HashMap<String, Uuid>,
) -> Result<Vec<Uuid>, AppError> {
    let mut ids = Vec::new();
    for n in names.iter().filter(|n| !n.trim().is_empty()) {
        let id = person(app, p, meta, n, cache).await?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

async fn write(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    r: &Row,
    cache: &mut HashMap<String, Uuid>,
) -> Result<Uuid, AppError> {
    let composers = people(app, p, meta, &r.composers, cache).await?;
    let lyricists = people(app, p, meta, &r.lyricists, cache).await?;
    let singers = people(app, p, meta, &r.singers, cache).await?;

    let mut credits: Vec<CreditInput> = composers
        .iter()
        .map(|a| CreditInput {
            artist_id: *a,
            role: "composer".into(),
        })
        .collect();
    credits.extend(lyricists.iter().map(|a| CreditInput {
        artist_id: *a,
        role: "lyricist".into(),
    }));
    let mut song = input(r);
    song.credits = Some(credits);
    let composition = cat::create_composition(app, p, meta, song).await?.composition.id;

    let recording = cat::create_recording(
        app,
        p,
        meta,
        RecordingInput {
            composition_id: Some(composition),
            title: Some(r.title.clone()),
            year: Some(r.year),
            credits: Some(
                singers
                    .iter()
                    .map(|a| CreditInput {
                        artist_id: *a,
                        role: "performer".into(),
                    })
                    .collect(),
            ),
            ..Default::default()
        },
    )
    .await?
    .recording
    .id;

    if let Some(title) = blank(&r.album) {
        let existing: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM albums WHERE lower(title) = lower($1) ORDER BY created_at LIMIT 1")
                .bind(&title)
                .fetch_optional(&app.pool)
                .await?;
        let album = match existing {
            Some(id) => id,
            None => {
                cat::create_album(
                    app,
                    p,
                    meta,
                    AlbumInput {
                        title: Some(title),
                        kind: Some(blank(&r.album_kind).unwrap_or_else(|| "album".into())),
                        release_year: Some(r.year),
                        ..Default::default()
                    },
                )
                .await?
                .album
                .id
            }
        };
        let mut tracks: Vec<Uuid> =
            sqlx::query_scalar("SELECT recording_id FROM album_tracks WHERE album_id = $1 ORDER BY position")
                .bind(album)
                .fetch_all(&app.pool)
                .await?;
        tracks.push(recording);
        let version: i32 = sqlx::query_scalar("SELECT version FROM albums WHERE id = $1")
            .bind(album)
            .fetch_one(&app.pool)
            .await?;
        cat::update_album(
            app,
            p,
            meta,
            album,
            Some(version),
            AlbumInput {
                tracks: Some(tracks),
                ..Default::default()
            },
        )
        .await?;
    }

    if let Some((language, script)) = language_and_script(app, r)
        .await?
        .filter(|_| !r.lyrics.trim().is_empty())
    {
        lyrics::create_contribution(
            app,
            p,
            meta,
            recording,
            None,
            ContributionInput {
                kind: "original".into(),
                language,
                script,
                body: r.lyrics.clone(),
                note: Some("Bulk import".into()),
                source_contribution_id: None,
            },
        )
        .await?;
    }
    Ok(composition)
}

fn problems_of(e: &AppError) -> Vec<(String, String)> {
    match e {
        AppError::Validation(v) => v.iter().map(|f| (f.field.to_owned(), f.code.to_owned())).collect(),
        AppError::Forbidden => vec![("row".into(), "forbidden".into())],
        _ => vec![("row".into(), "internal".into())],
    }
}

/// Imports (or, with `dry_run`, only checks) the rows. A row that fails does not stop the others.
/// Moderators and admins only.
pub async fn songs(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    rows: Vec<Row>,
    dry_run: bool,
) -> Result<Report, AppError> {
    authz::require_role(p, Role::Moderator)?;
    if rows.is_empty() {
        return Err(domain::DomainError::invalid("rows", "required").into());
    }
    if rows.len() > MAX_ROWS {
        return Err(domain::DomainError::invalid("rows", "too_many").into());
    }
    let mut cache: HashMap<String, Uuid> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut results = Vec::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        let mut res = RowResult {
            row: i + 1,
            title: r.title.trim().to_owned(),
            status: Status::Failed,
            reason: None,
            problems: Vec::new(),
            composition_id: None,
        };
        let problems = check(app, r).await?;
        if !problems.is_empty() {
            res.problems = problems;
        } else if duplicate(app, r, &seen).await? {
            res.status = Status::Skipped;
            res.reason = Some("already_exists".into());
        } else if dry_run {
            res.status = Status::WouldCreate;
            seen.insert((r.title.trim().to_lowercase(), r.album.trim().to_lowercase()));
        } else {
            match write(app, p, meta, r, &mut cache).await {
                Ok(id) => {
                    res.status = Status::Created;
                    res.composition_id = Some(id);
                    seen.insert((r.title.trim().to_lowercase(), r.album.trim().to_lowercase()));
                }
                Err(e @ AppError::Internal(_)) => {
                    tracing::error!(error = %e, row = i + 1, "bulk import row failed");
                    res.problems = problems_of(&e);
                }
                Err(e) => res.problems = problems_of(&e),
            }
        }
        results.push(res);
    }
    Ok(Report { dry_run, results })
}
