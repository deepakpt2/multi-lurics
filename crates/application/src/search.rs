//! Public search. `SearchService` is the only seam to the engine: it ranks and returns ids, and
//! nothing about the engine (scores, trigrams, index names) reaches the API. The Postgres
//! implementation reads `search_documents`, the published-only read model the outbox worker keeps
//! current. Display text, localization and context come from the catalogue, the same way every
//! other read does, so a swapped engine cannot change what is shown.

use crate::localization::{names, resolve_requested, Display};
use crate::{authz, App, AppError};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use domain::localization::EntityType;
use domain::{Principal, Role, Violations};
use infrastructure::search as repo;
use sqlx::PgPool;
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

pub const SEARCH_DEFAULT: i64 = 10;
pub const SEARCH_MAX: i64 = 50;
pub const SUGGEST_DEFAULT: i64 = 8;
pub const SUGGEST_MAX: i64 = 20;
const MAX_QUERY_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Compositions,
    Recordings,
    Artists,
    Albums,
    /// Artists credited as writers or translators of a composition.
    Contributors,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Self::Compositions,
        Self::Recordings,
        Self::Artists,
        Self::Albums,
        Self::Contributors,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compositions => "compositions",
            Self::Recordings => "recordings",
            Self::Artists => "artists",
            Self::Albums => "albums",
            Self::Contributors => "contributors",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.as_str() == s)
    }

    fn entity(self) -> SearchType {
        match self {
            Self::Compositions => SearchType::Composition,
            Self::Recordings => SearchType::Recording,
            Self::Artists | Self::Contributors => SearchType::Artist,
            Self::Albums => SearchType::Album,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchType {
    Composition,
    Recording,
    Artist,
    Album,
}

impl SearchType {
    pub const ALL: [SearchType; 4] = [Self::Composition, Self::Recording, Self::Artist, Self::Album];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Composition => "composition",
            Self::Recording => "recording",
            Self::Artist => "artist",
            Self::Album => "album",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    fn entity(self) -> EntityType {
        match self {
            Self::Composition => EntityType::Composition,
            Self::Recording => EntityType::Recording,
            Self::Artist => EntityType::Artist,
            Self::Album => EntityType::Album,
        }
    }
}

/// How the query related to the matched name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Exact,
    Normalized,
    Prefix,
    Fuzzy,
}

impl MatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Normalized => "normalized",
            Self::Prefix => "prefix",
            Self::Fuzzy => "fuzzy",
        }
    }
}

/// Which name matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchVia {
    Name,
    Localization,
    Transliteration,
    Alias,
    Metadata,
    /// The words of a published lyric.
    Lyrics,
}

impl MatchVia {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Localization => "localization",
            Self::Transliteration => "transliteration",
            Self::Alias => "alias",
            Self::Metadata => "metadata",
            Self::Lyrics => "lyrics",
        }
    }
}

/// Where a page ended; opaque to callers, handed back to continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position(pub i64, pub Uuid);

#[derive(Debug, Clone)]
pub struct Hit {
    pub entity: SearchType,
    pub id: Uuid,
    /// The matched name as written.
    pub text: String,
    pub kind: MatchKind,
    pub via: MatchVia,
    pub position: Position,
}

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub q: String,
    /// Which entity types to search. `contributors_only` narrows artists to credited writers/translators.
    pub types: Vec<SearchType>,
    pub contributors_only: bool,
    pub language_id: Option<Uuid>,
    pub limit: i64,
    pub after: Option<Position>,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchSettings {
    pub min_suggest_chars: usize,
    pub cache_max_age_secs: u32,
}

#[async_trait]
pub trait SearchService: Send + Sync {
    /// Public, published-only hits, best first, at most one per entity.
    async fn search(&self, req: &SearchRequest) -> Result<Vec<Hit>, AppError>;
    async fn settings(&self) -> Result<SearchSettings, AppError>;
}

/// The Postgres implementation: exact → normalized → prefix → trigram-fuzzy over `search_documents`,
/// weighted by the `search_ranking` setting.
pub struct PgSearch {
    pool: PgPool,
}

impl PgSearch {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SearchService for PgSearch {
    async fn search(&self, req: &SearchRequest) -> Result<Vec<Hit>, AppError> {
        let mut conn = self.pool.acquire().await?;
        let ranking = repo::load_ranking(&mut conn).await?;
        let types: Vec<&str> = req.types.iter().map(|t| t.as_str()).collect();
        let rows = repo::search(
            &mut conn,
            &ranking,
            &repo::Query {
                q: &req.q,
                types: &types,
                contributors_only: req.contributors_only,
                language_id: req.language_id,
                limit: req.limit,
                after: req.after.map(|p| (p.0, p.1)),
            },
        )
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(Hit {
                    entity: SearchType::parse(&r.entity_type)?,
                    id: r.entity_id,
                    text: r.display,
                    kind: match r.class.as_str() {
                        "exact" => MatchKind::Exact,
                        "normalized" => MatchKind::Normalized,
                        "prefix" | "lyric_opening" | "lyric_line" => MatchKind::Prefix,
                        _ => MatchKind::Fuzzy,
                    },
                    via: match r.origin.as_str() {
                        "localization" => MatchVia::Localization,
                        "transliteration" => MatchVia::Transliteration,
                        "alias" => MatchVia::Alias,
                        "metadata" => MatchVia::Metadata,
                        "lyrics" => MatchVia::Lyrics,
                        _ => MatchVia::Name,
                    },
                    position: Position(r.score, r.entity_id),
                })
            })
            .collect())
    }

    async fn settings(&self) -> Result<SearchSettings, AppError> {
        let mut conn = self.pool.acquire().await?;
        let r = repo::load_ranking(&mut conn).await?;
        Ok(SearchSettings {
            min_suggest_chars: r.min_suggest_chars,
            cache_max_age_secs: r.cache_max_age_secs,
        })
    }
}

// ---------- results ----------

#[derive(Debug, Clone)]
pub struct Matched {
    pub text: String,
    pub kind: MatchKind,
    pub via: MatchVia,
}

#[derive(Debug, Clone)]
pub struct Parent {
    pub id: Uuid,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub entity: SearchType,
    pub id: Uuid,
    pub display: Display,
    pub matched: Matched,
    /// Artist kind, album kind.
    pub sub_kind: Option<String>,
    /// Artist disambiguation, composition ISWC.
    pub detail: Option<String>,
    pub year: Option<i16>,
    /// A recording's composition.
    pub parent: Option<Parent>,
}

#[derive(Debug, Clone)]
pub struct GroupResult {
    pub group: Group,
    pub items: Vec<Item>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Results {
    pub groups: Vec<GroupResult>,
    pub max_age_secs: u32,
}

fn encode_cursor(p: Position) -> String {
    URL_SAFE_NO_PAD.encode(format!("s1|{}|{}", p.0, p.1))
}

fn decode_cursor(s: &str) -> Result<Position, AppError> {
    let bad = || AppError::BadRequest("invalid_cursor");
    if s.len() > 128 {
        return Err(bad());
    }
    let bytes = URL_SAFE_NO_PAD.decode(s).map_err(|_| bad())?;
    let text = std::str::from_utf8(&bytes).map_err(|_| bad())?;
    let mut parts = text.split('|');
    let (Some("s1"), Some(score), Some(id), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(bad());
    };
    Ok(Position(
        score.parse().map_err(|_| bad())?,
        Uuid::parse_str(id).map_err(|_| bad())?,
    ))
}

fn clean_query(v: &mut Violations, raw: &str) -> String {
    let q = raw.trim();
    if q.is_empty() {
        v.push("q", "required");
    } else if q.chars().count() > MAX_QUERY_CHARS {
        v.push("q", "too_long");
    }
    q.to_owned()
}

fn limit_of(v: &mut Violations, requested: Option<i64>, default: i64, max: i64) -> i64 {
    match requested {
        None => default,
        Some(n) if (1..=max).contains(&n) => n,
        Some(_) => {
            v.push("limit", "out_of_range");
            default
        }
    }
}

/// Turns ranked hits into displayable items. Hits whose entity no longer exists (merged away or
/// deleted since indexing) are dropped here, so a stale index row can never surface.
async fn describe(app: &App, hits: &[Hit], lang: Option<&str>) -> Result<Vec<Item>, AppError> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let mut conn = app.pool.acquire().await?;
    let req = resolve_requested(&mut conn, lang).await?;
    let mut facts: HashMap<(SearchType, Uuid), repo::Fact> = HashMap::new();
    for t in SearchType::ALL {
        let ids: Vec<Uuid> = hits.iter().filter(|h| h.entity == t).map(|h| h.id).collect();
        if ids.is_empty() {
            continue;
        }
        for f in repo::facts(&mut conn, t.as_str(), &ids).await? {
            facts.insert((t, f.id), f);
        }
    }
    let refs: Vec<_> = facts
        .iter()
        .map(|((t, id), f)| (t.entity(), *id, f.title.clone()))
        .collect();
    let shown = names(&mut conn, &req, &refs).await?;
    Ok(hits
        .iter()
        .filter_map(|h| {
            let f = facts.get(&(h.entity, h.id))?;
            Some(Item {
                entity: h.entity,
                id: h.id,
                display: shown.get(h.entity.entity(), h.id, &f.title),
                matched: Matched {
                    text: h.text.clone(),
                    kind: h.kind,
                    via: h.via,
                },
                sub_kind: f.sub_kind.clone(),
                detail: f.detail.clone(),
                year: f.year,
                parent: f
                    .parent_id
                    .zip(f.parent_title.clone())
                    .map(|(id, title)| Parent { id, title }),
            })
        })
        .collect())
}

/// Counts the request without delaying it or failing it.
fn note(app: &App, endpoint: &'static str, started: Instant, empty: bool, accepted: bool) {
    let pool = app.pool.clone();
    let ms = started.elapsed().as_millis() as i64;
    let shard = (Uuid::new_v4().as_u128() % 8) as i16;
    tokio::spawn(async move {
        let result = async {
            let mut conn = pool.acquire().await?;
            repo::record_metric(&mut conn, endpoint, shard, empty, ms, accepted).await
        }
        .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "search metric not recorded");
        }
    });
}

/// `GET /search`: typed groups. With `group` set the one group is paged by `cursor`; without it every
/// group returns its first `limit` hits.
pub async fn search(
    app: &App,
    q: &str,
    group: Option<&str>,
    lang: Option<&str>,
    limit: Option<i64>,
    cursor: Option<&str>,
) -> Result<Results, AppError> {
    let started = Instant::now();
    let mut v = Violations::default();
    let q = clean_query(&mut v, q);
    let group = match group {
        None => None,
        Some(g) => {
            let parsed = Group::parse(g);
            if parsed.is_none() {
                v.push("type", "invalid_choice");
            }
            parsed
        }
    };
    let limit = limit_of(&mut v, limit, SEARCH_DEFAULT, SEARCH_MAX);
    v.finish()?;
    if cursor.is_some() && group.is_none() {
        return Err(AppError::BadRequest("cursor_requires_type"));
    }
    let after = cursor.map(decode_cursor).transpose()?;
    let settings = app.search.settings().await?;

    let language_id = {
        let mut conn = app.pool.acquire().await?;
        resolve_requested(&mut conn, lang).await?.language_id
    };
    let wanted: Vec<Group> = group.map(|g| vec![g]).unwrap_or_else(|| Group::ALL.to_vec());
    let mut groups = Vec::new();
    let mut any = false;
    for g in wanted {
        // One extra row tells whether another page exists.
        let hits = app
            .search
            .search(&SearchRequest {
                q: q.clone(),
                types: vec![g.entity()],
                contributors_only: g == Group::Contributors,
                language_id,
                limit: limit + 1,
                after,
            })
            .await?;
        let more = hits.len() as i64 > limit;
        let page: Vec<Hit> = hits.into_iter().take(limit as usize).collect();
        let next_cursor = more.then(|| page.last().map(|h| encode_cursor(h.position))).flatten();
        let items = describe(app, &page, lang).await?;
        any |= !items.is_empty();
        groups.push(GroupResult {
            group: g,
            items,
            next_cursor,
        });
    }
    note(app, "search", started, !any, false);
    Ok(Results {
        groups,
        max_age_secs: settings.cache_max_age_secs,
    })
}

/// `GET /search/suggest`: one flat, mixed-type list for typeahead.
pub async fn suggest(app: &App, q: &str, lang: Option<&str>, limit: Option<i64>) -> Result<(Vec<Item>, u32), AppError> {
    let started = Instant::now();
    let mut v = Violations::default();
    let q = clean_query(&mut v, q);
    let limit = limit_of(&mut v, limit, SUGGEST_DEFAULT, SUGGEST_MAX);
    let settings = app.search.settings().await?;
    if !q.is_empty() && q.chars().count() < settings.min_suggest_chars {
        v.push("q", "too_short");
    }
    v.finish()?;
    let language_id = {
        let mut conn = app.pool.acquire().await?;
        resolve_requested(&mut conn, lang).await?.language_id
    };
    let hits = app
        .search
        .search(&SearchRequest {
            q,
            types: SearchType::ALL.to_vec(),
            contributors_only: false,
            language_id,
            limit,
            after: None,
        })
        .await?;
    let items = describe(app, &hits, lang).await?;
    note(app, "suggest", started, items.is_empty(), false);
    Ok((items, settings.cache_max_age_secs))
}

/// The client reports that a suggestion was chosen. Only a counter moves.
pub fn suggestion_accepted(app: &App) {
    note(app, "suggest", Instant::now(), false, true);
}

#[derive(Debug, Clone)]
pub struct MetricsRow {
    pub day: chrono::NaiveDate,
    pub endpoint: String,
    pub requests: i64,
    pub empty: i64,
    pub avg_latency_ms: f64,
    pub accepted: i64,
}

/// Admin-only usage summary: latency, empty-result rate and (for suggest) acceptance.
pub async fn metrics(app: &App, p: &Principal, days: Option<i32>) -> Result<Vec<MetricsRow>, AppError> {
    authz::require_role(p, Role::Admin)?;
    let days = match days {
        None => 7,
        Some(n) if (1..=90).contains(&n) => n,
        Some(_) => return Err(domain::DomainError::invalid("days", "out_of_range").into()),
    };
    let mut conn = app.pool.acquire().await?;
    Ok(repo::metrics(&mut conn, days)
        .await?
        .into_iter()
        .map(|m| MetricsRow {
            day: m.day,
            endpoint: m.endpoint,
            requests: m.requests,
            empty: m.empty,
            avg_latency_ms: if m.requests > 0 {
                m.latency_ms as f64 / m.requests as f64
            } else {
                0.0
            },
            accepted: m.accepted,
        })
        .collect())
}
