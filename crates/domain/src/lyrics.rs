//! Lyric contributions: types, review states, body validation, trust policy and line diff.

use crate::{DomainError, Principal, Role};

macro_rules! closed_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $s),+ } }
            pub fn parse(s: &str) -> Option<Self> { match s { $($s => Some(Self::$variant),)+ _ => None } }
        }
    };
}

closed_enum!(ContributionType {
    Original => "original", Translation => "translation", Transliteration => "transliteration",
});

closed_enum!(
    /// State of a contribution's head revision. The public sees `published_revision_id` only.
    Status { Pending => "pending", Approved => "approved", Published => "published", Rejected => "rejected" }
);

pub const MAX_BODY_CHARS: usize = 100_000;
pub const MAX_NOTE_CHARS: usize = 500;

/// Lyrics are stored as written, except that line endings are normalised to `\n` so that
/// revisions and diffs do not depend on the author's platform. NUL and other control
/// characters (apart from tab/newline) are refused; bidi marks and ZWJ/ZWNJ are kept.
pub fn body(raw: &str) -> Result<String, DomainError> {
    let s = raw.replace("\r\n", "\n").replace('\r', "\n");
    if s.trim().is_empty() {
        return Err(DomainError::invalid("body", "required"));
    }
    if s.chars().count() > MAX_BODY_CHARS {
        return Err(DomainError::invalid("body", "too_long"));
    }
    if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(DomainError::invalid("body", "invalid_characters"));
    }
    Ok(s)
}

/// Optional single-line note (edit summary, review reason).
pub fn note(field: &'static str, raw: Option<&str>, max: usize) -> Result<Option<String>, DomainError> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) if s.chars().count() > max => Err(DomainError::invalid(field, "too_long")),
        Some(s) if s.chars().any(char::is_control) => Err(DomainError::invalid(field, "invalid_characters")),
        Some(s) => Ok(Some(s.to_owned())),
    }
}

/// `Idempotency-Key`: 8–128 printable characters from a conservative alphabet.
pub fn idempotency_key(raw: &str) -> Result<&str, DomainError> {
    let ok = (8..=128).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'));
    if ok {
        Ok(raw)
    } else {
        Err(DomainError::invalid("idempotency_key", "invalid"))
    }
}

/// Who skips the review queue. Loaded from the `settings` table, never hardcoded.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TrustPolicy {
    /// Role names (`moderator`, `admin`) whose holders publish directly.
    pub auto_publish_roles: Vec<String>,
    /// Members with at least this many published contributions publish directly. `null` = off.
    pub auto_publish_min_published: Option<i64>,
}

impl TrustPolicy {
    pub fn validate(&self) -> Result<(), DomainError> {
        if self
            .auto_publish_roles
            .iter()
            .any(|r| !matches!(r.as_str(), "moderator" | "admin"))
        {
            return Err(DomainError::invalid("auto_publish_roles", "invalid_choice"));
        }
        if self
            .auto_publish_min_published
            .is_some_and(|n| !(1..=100_000).contains(&n))
        {
            return Err(DomainError::invalid("auto_publish_min_published", "out_of_range"));
        }
        Ok(())
    }

    /// True when `p`'s submission is published without review.
    pub fn auto_publishes(&self, p: &Principal, published_so_far: i64) -> bool {
        let by_role = self.auto_publish_roles.iter().any(|r| match r.as_str() {
            "moderator" => p.has_role(Role::Moderator),
            "admin" => p.has_role(Role::Admin),
            _ => false,
        });
        by_role || self.auto_publish_min_published.is_some_and(|n| published_so_far >= n)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Hunk {
    pub op: Op,
    pub lines: Vec<String>,
}

/// Work bound for the quadratic middle part; larger changes degrade to delete-all/insert-all.
const MAX_CELLS: usize = 4_000_000;

/// Line diff from `a` to `b` (longest common subsequence after trimming the shared head and tail).
pub fn diff(a: &str, b: &str) -> Vec<Hunk> {
    let a: Vec<&str> = a.split('\n').collect();
    let b: Vec<&str> = b.split('\n').collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (ma, mb) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);

    let mut ops: Vec<(Op, &str)> = a[..head].iter().map(|l| (Op::Equal, *l)).collect();
    if (ma.len() + 1) * (mb.len() + 1) > MAX_CELLS {
        ops.extend(ma.iter().map(|l| (Op::Delete, *l)));
        ops.extend(mb.iter().map(|l| (Op::Insert, *l)));
    } else {
        let w = mb.len() + 1;
        let mut t = vec![0u32; (ma.len() + 1) * w];
        for i in (0..ma.len()).rev() {
            for j in (0..mb.len()).rev() {
                t[i * w + j] = if ma[i] == mb[j] {
                    t[(i + 1) * w + j + 1] + 1
                } else {
                    t[(i + 1) * w + j].max(t[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < ma.len() && j < mb.len() {
            if ma[i] == mb[j] {
                ops.push((Op::Equal, ma[i]));
                i += 1;
                j += 1;
            } else if t[(i + 1) * w + j] >= t[i * w + j + 1] {
                ops.push((Op::Delete, ma[i]));
                i += 1;
            } else {
                ops.push((Op::Insert, mb[j]));
                j += 1;
            }
        }
        ops.extend(ma[i..].iter().map(|l| (Op::Delete, *l)));
        ops.extend(mb[j..].iter().map(|l| (Op::Insert, *l)));
    }
    ops.extend(a[a.len() - tail..].iter().map(|l| (Op::Equal, *l)));

    let mut hunks: Vec<Hunk> = Vec::new();
    for (op, line) in ops {
        match hunks.last_mut() {
            Some(h) if h.op == op => h.lines.push(line.to_owned()),
            _ => hunks.push(Hunk {
                op,
                lines: vec![line.to_owned()],
            }),
        }
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(roles: Vec<Role>) -> Principal {
        Principal {
            user_id: uuid::Uuid::nil(),
            session_id: uuid::Uuid::nil(),
            roles,
        }
    }

    #[test]
    fn body_normalises_newlines_and_keeps_unicode_marks() {
        assert_eq!(body("a\r\nb\rc").unwrap(), "a\nb\nc");
        let s = "ക\u{200d}ല\u{200f} abc\u{202b}";
        assert_eq!(body(s).unwrap(), s);
        assert!(body("  \n ").is_err());
        assert!(body("a\0b").is_err());
        assert!(body(&"x".repeat(MAX_BODY_CHARS + 1)).is_err());
    }

    #[test]
    fn diff_marks_inserted_deleted_and_equal_lines() {
        let h = diff("a\nb\nc", "a\nx\nc\nd");
        let shape: Vec<_> = h.iter().map(|h| (h.op, h.lines.clone())).collect();
        assert_eq!(
            shape,
            vec![
                (Op::Equal, vec!["a".into()]),
                (Op::Delete, vec!["b".into()]),
                (Op::Insert, vec!["x".into()]),
                (Op::Equal, vec!["c".into()]),
                (Op::Insert, vec!["d".into()]),
            ]
        );
        assert_eq!(
            diff("same", "same"),
            vec![Hunk {
                op: Op::Equal,
                lines: vec!["same".into()]
            }]
        );
    }

    #[test]
    fn trust_policy_by_role_and_by_track_record() {
        let pol = TrustPolicy {
            auto_publish_roles: vec!["moderator".into()],
            auto_publish_min_published: Some(3),
        };
        assert!(pol.auto_publishes(&p(vec![Role::Moderator]), 0));
        assert!(pol.auto_publishes(&p(vec![Role::Admin]), 0));
        assert!(!pol.auto_publishes(&p(vec![]), 2));
        assert!(pol.auto_publishes(&p(vec![]), 3));
        let bad = TrustPolicy {
            auto_publish_roles: vec!["root".into()],
            auto_publish_min_published: None,
        };
        assert!(bad.validate().is_err());
    }
}
