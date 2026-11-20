//! "Contribute a song": one submission from any signed-in member makes the song, its people, its album
//! and track, and one lyric contribution per language/type block, so nobody has to know that lyrics
//! hang off a recording. Lyrics go through the ordinary review rules (moderators publish at once, others
//! wait for review). A translation or transliteration needs no original: it can come first.

use crate::catalogue::{self as cat, AlbumInput, CompositionInput, CreditInput, RecordingInput};
use crate::import::{language_and_script_of, person};
use crate::lyrics::{self, ContributionInput};
use crate::{App, AppError, RequestMeta};
use domain::Principal;
use std::collections::HashMap;
use uuid::Uuid;

/// Something that may already exist: chosen from suggestions (`id`) or typed (`name`).
#[derive(Debug, Clone, Default)]
pub struct NameRef {
    pub id: Option<Uuid>,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct LyricBlock {
    pub kind: String,
    pub language: String,
    pub script: String,
    pub body: String,
}

#[derive(Debug, Clone, Default)]
pub struct Submission {
    pub title: String,
    pub year: Option<i32>,
    pub album: Option<NameRef>,
    pub singers: Vec<NameRef>,
    pub composers: Vec<NameRef>,
    pub lyricists: Vec<NameRef>,
    pub category: String,
    pub religion: String,
    pub deity: String,
    pub raga: String,
    pub devotional_form: String,
    pub source_work: String,
    pub occasions: Vec<String>,
    pub tags: Vec<String>,
    pub youtube: String,
    pub lyrics: Vec<LyricBlock>,
    /// Make the song even if one with this title is already on the album.
    pub allow_duplicate: bool,
}

#[derive(Debug, Clone)]
pub struct Made {
    pub composition_id: Uuid,
    pub recording_id: Uuid,
    /// `published` or `pending`, per lyric block, in order.
    pub statuses: Vec<String>,
}

fn blank(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

async fn who(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    r: &NameRef,
    cache: &mut HashMap<String, Uuid>,
) -> Result<Uuid, AppError> {
    if let Some(id) = r.id {
        let there: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM artists WHERE id = $1)")
            .bind(id)
            .fetch_one(&app.pool)
            .await?;
        if there {
            return Ok(id);
        }
    }
    person(app, p, meta, &r.name, cache).await
}

async fn people(
    app: &App,
    p: &Principal,
    meta: &RequestMeta,
    refs: &[NameRef],
    cache: &mut HashMap<String, Uuid>,
) -> Result<Vec<Uuid>, AppError> {
    let mut out = Vec::new();
    for r in refs.iter().filter(|r| r.id.is_some() || !r.name.trim().is_empty()) {
        let id = who(app, p, meta, r, cache).await?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

pub async fn song(app: &App, p: &Principal, meta: &RequestMeta, s: Submission) -> Result<Made, AppError> {
    let devotional = s.category.trim() == "devotional";
    let input = CompositionInput {
        title: Some(s.title.clone()),
        year: Some(s.year),
        category: blank(&s.category),
        raga: Some(blank(&s.raga)),
        religion: devotional.then(|| blank(&s.religion)),
        deity: devotional.then(|| blank(&s.deity)),
        devotional_form: devotional.then(|| blank(&s.devotional_form)),
        source_work: devotional.then(|| blank(&s.source_work)),
        occasions: devotional.then(|| s.occasions.clone()),
        tags: Some(s.tags.clone()),
        youtube: Some(blank(&s.youtube)),
        ..Default::default()
    };
    // Everything that can be known without writing is checked first.
    cat::validate_composition(&input)?;
    let blocks: Vec<&LyricBlock> = s.lyrics.iter().filter(|b| !b.body.trim().is_empty()).collect();
    if blocks.is_empty() {
        return Err(domain::DomainError::invalid("lyrics", "required").into());
    }
    let mut resolved = Vec::new();
    for b in &blocks {
        if !matches!(b.kind.trim(), "original" | "translation" | "transliteration") {
            return Err(domain::DomainError::invalid("type", "invalid_choice").into());
        }
        domain::lyrics::body(&b.body)?;
        match language_and_script_of(app, &b.language, &b.script).await? {
            Some(ls) => resolved.push(ls),
            None => {
                return Err(domain::DomainError::invalid(
                    "language",
                    if b.language.trim().is_empty() {
                        "required"
                    } else {
                        "unknown_language"
                    },
                )
                .into())
            }
        }
    }

    // A song with this title already on the album: ask, do not guess.
    if !s.allow_duplicate {
        if let Some(a) = &s.album {
            let title = s.title.trim().to_lowercase();
            let found: Vec<Uuid> = sqlx::query_scalar(
                "SELECT DISTINCT c.id FROM compositions c JOIN recordings r ON r.composition_id = c.id
                   JOIN album_tracks t ON t.recording_id = r.id JOIN albums al ON al.id = t.album_id
                  WHERE lower(c.title) = $1 AND (al.id = $2 OR lower(al.title) = $3)",
            )
            .bind(&title)
            .bind(a.id.unwrap_or_else(Uuid::nil))
            .bind(a.name.trim().to_lowercase())
            .fetch_all(&app.pool)
            .await?;
            if !found.is_empty() {
                return Err(AppError::Ambiguous {
                    code: "possible_duplicate",
                    candidates: found,
                });
            }
        }
    }

    let mut cache = HashMap::new();
    let composers = people(app, p, meta, &s.composers, &mut cache).await?;
    let lyricists = people(app, p, meta, &s.lyricists, &mut cache).await?;
    let singers = people(app, p, meta, &s.singers, &mut cache).await?;
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
    let mut comp = input;
    comp.credits = Some(credits);
    let composition = cat::create_composition(app, p, meta, comp).await?.composition.id;
    let recording = cat::create_recording(
        app,
        p,
        meta,
        RecordingInput {
            composition_id: Some(composition),
            title: Some(s.title.clone()),
            year: Some(s.year),
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

    if let Some(a) = s.album.as_ref().filter(|a| a.id.is_some() || !a.name.trim().is_empty()) {
        let existing: Option<Uuid> = match a.id {
            Some(id) => {
                sqlx::query_scalar("SELECT id FROM albums WHERE id = $1")
                    .bind(id)
                    .fetch_optional(&app.pool)
                    .await?
            }
            None => None,
        };
        let existing = match existing {
            Some(id) => Some(id),
            None => {
                sqlx::query_scalar("SELECT id FROM albums WHERE lower(title) = lower($1) ORDER BY created_at LIMIT 1")
                    .bind(a.name.trim())
                    .fetch_optional(&app.pool)
                    .await?
            }
        };
        let album = match existing {
            Some(id) => id,
            None => {
                cat::create_album(
                    app,
                    p,
                    meta,
                    AlbumInput {
                        title: Some(a.name.trim().to_owned()),
                        kind: Some("album".into()),
                        release_year: Some(s.year),
                        ..Default::default()
                    },
                )
                .await?
                .album
                .id
            }
        };
        // Appending a track is open to every contributor; rewriting an album's track list is not.
        sqlx::query(
            "INSERT INTO album_tracks (album_id, position, recording_id)
             SELECT $1, coalesce(max(position), 0) + 1, $2 FROM album_tracks WHERE album_id = $1",
        )
        .bind(album)
        .bind(recording)
        .execute(&app.pool)
        .await?;
    }

    let mut statuses = Vec::new();
    for (b, (language, script)) in blocks.iter().zip(resolved) {
        let out = lyrics::create_contribution(
            app,
            p,
            meta,
            recording,
            None,
            ContributionInput {
                kind: b.kind.trim().to_owned(),
                language,
                script,
                body: b.body.clone(),
                note: None,
                source_contribution_id: None,
            },
        )
        .await?;
        statuses.push(out.value.row.status.clone());
    }
    Ok(Made {
        composition_id: composition,
        recording_id: recording,
        statuses,
    })
}
