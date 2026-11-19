//! Opaque keyset cursors. Clients must treat cursors as opaque strings.

use crate::AppError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const DEFAULT_LIMIT: i64 = 25;
pub const MAX_LIMIT: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub ts: DateTime<Utc>,
    pub id: Uuid,
}

impl Cursor {
    const PREFIX: &'static str = "v1";

    pub fn encode(&self) -> String {
        let raw = format!("{}|{}|{}", Self::PREFIX, self.ts.timestamp_micros(), self.id);
        URL_SAFE_NO_PAD.encode(raw)
    }

    pub fn decode(s: &str) -> Result<Self, AppError> {
        let bad = || AppError::BadRequest("invalid_cursor");
        if s.len() > 128 {
            return Err(bad());
        }
        let bytes = URL_SAFE_NO_PAD.decode(s).map_err(|_| bad())?;
        let text = std::str::from_utf8(&bytes).map_err(|_| bad())?;
        let mut parts = text.split('|');
        let (Some(Self::PREFIX), Some(micros), Some(id), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        let ts = micros
            .parse()
            .ok()
            .and_then(DateTime::from_timestamp_micros)
            .ok_or_else(bad)?;
        let id = Uuid::parse_str(id).map_err(|_| bad())?;
        Ok(Self { ts, id })
    }
}

/// Validates the requested page size; out-of-range is a field error, not a silent clamp.
pub fn limit(requested: Option<i64>) -> Result<i64, AppError> {
    match requested {
        None => Ok(DEFAULT_LIMIT),
        Some(n) if (1..=MAX_LIMIT).contains(&n) => Ok(n),
        Some(_) => Err(domain::DomainError::invalid("limit", "out_of_range").into()),
    }
}

/// A page of results plus the cursor for the next page (`None` on the last page).
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    /// `rows` must have been fetched with `limit + 1` to detect a following page.
    pub fn from_overfetch(mut rows: Vec<T>, limit: i64, key: impl Fn(&T) -> Cursor) -> Self {
        let has_more = rows.len() as i64 > limit;
        rows.truncate(limit as usize);
        let next_cursor = if has_more {
            rows.last().map(|r| key(r).encode())
        } else {
            None
        };
        Self {
            items: rows,
            next_cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrips_with_microsecond_precision() {
        let c = Cursor {
            ts: DateTime::from_timestamp_micros(1_763_500_000_123_456).unwrap(),
            id: Uuid::new_v4(),
        };
        assert_eq!(Cursor::decode(&c.encode()).unwrap(), c);
    }

    #[test]
    fn tampered_or_garbage_cursors_are_rejected() {
        let good = Cursor {
            ts: Utc::now(),
            id: Uuid::new_v4(),
        }
        .encode();
        let bad_inputs = [
            String::new(),
            "!!!".into(),
            URL_SAFE_NO_PAD.encode("v2|1|00000000-0000-0000-0000-000000000000"),
            URL_SAFE_NO_PAD.encode("v1|abc|00000000-0000-0000-0000-000000000000"),
            URL_SAFE_NO_PAD.encode("v1|1|not-a-uuid"),
            URL_SAFE_NO_PAD.encode("v1|1|00000000-0000-0000-0000-000000000000|extra"),
            format!("{good}x"),
            "a".repeat(200),
        ];
        for s in bad_inputs {
            assert!(
                matches!(Cursor::decode(&s), Err(AppError::BadRequest("invalid_cursor"))),
                "{s}"
            );
        }
    }

    #[test]
    fn limit_bounds() {
        assert_eq!(limit(None).unwrap(), DEFAULT_LIMIT);
        assert_eq!(limit(Some(100)).unwrap(), 100);
        assert!(matches!(limit(Some(0)), Err(AppError::Validation(_))));
        assert!(matches!(limit(Some(101)), Err(AppError::Validation(_))));
    }

    #[test]
    fn overfetch_sets_next_cursor_only_when_more_rows_exist() {
        let key = |n: &i64| Cursor {
            ts: DateTime::from_timestamp(*n, 0).unwrap(),
            id: Uuid::nil(),
        };
        let p = Page::from_overfetch(vec![3, 2, 1], 2, key);
        assert_eq!(p.items, vec![3, 2]);
        assert_eq!(Cursor::decode(p.next_cursor.as_deref().unwrap()).unwrap(), key(&2));
        let last = Page::from_overfetch(vec![1], 2, key);
        assert!(last.next_cursor.is_none());
    }
}
