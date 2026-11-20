//! Community rules that need no I/O: the tunable settings, text limits and spam heuristics.
//! Reputation is a score derived from an event log; it gates community actions but is never a role.

use crate::{DomainError, Violations};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_PANELS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Level {
    pub name: String,
    pub min: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Limits {
    pub comment_chars: usize,
    pub bio_chars: usize,
    pub report_detail_chars: usize,
    pub appeal_chars: usize,
    pub note_chars: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Rate {
    pub max: u32,
    pub window_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Spam {
    pub max_links: usize,
    pub max_repeat_run: usize,
    pub duplicate_window_secs: u32,
    pub blocked_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommunitySettings {
    /// Event kind -> points. Unknown kinds score nothing.
    pub points: BTreeMap<String, i64>,
    /// Action -> minimum reputation needed (`rate`, `react`, `comment`, `report`, `appeal`).
    pub min_reputation: BTreeMap<String, i64>,
    pub levels: Vec<Level>,
    pub limits: Limits,
    pub rate: BTreeMap<String, Rate>,
    pub spam: Spam,
}

impl CommunitySettings {
    pub fn validate(&self) -> Result<(), DomainError> {
        let mut v = Violations::default();
        if self.levels.is_empty() || self.levels.windows(2).any(|w| w[0].min >= w[1].min) {
            v.push("levels", "invalid_choice");
        }
        let l = &self.limits;
        for (field, n, max) in [
            ("limits.comment_chars", l.comment_chars, 5000),
            ("limits.bio_chars", l.bio_chars, 1000),
            ("limits.report_detail_chars", l.report_detail_chars, 2000),
            ("limits.appeal_chars", l.appeal_chars, 2000),
            ("limits.note_chars", l.note_chars, 2000),
        ] {
            if n == 0 || n > max {
                v.push(field, "out_of_range");
            }
        }
        if self
            .rate
            .values()
            .any(|r| r.max == 0 || r.window_secs == 0 || r.window_secs > 86_400)
        {
            v.push("rate", "out_of_range");
        }
        if self.min_reputation.values().any(|m| *m < 0) {
            v.push("min_reputation", "out_of_range");
        }
        if self.spam.blocked_terms.len() > 500 || self.spam.blocked_terms.iter().any(|t| t.trim().is_empty()) {
            v.push("spam.blocked_terms", "invalid_choice");
        }
        v.finish()
    }

    pub fn level_for(&self, reputation: i64) -> &str {
        self.levels
            .iter()
            .rev()
            .find(|l| reputation >= l.min)
            .map_or("", |l| l.name.as_str())
    }

    pub fn points_for(&self, kind: &str) -> i64 {
        self.points.get(kind).copied().unwrap_or(0)
    }
}

/// Trimmed, non-empty, within `max` characters (counted as characters, not bytes).
pub fn text(field: &'static str, raw: &str, max: usize) -> Result<String, DomainError> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(DomainError::invalid(field, "required"));
    }
    if t.chars().count() > max {
        return Err(DomainError::invalid(field, "too_long"));
    }
    if t.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err(DomainError::invalid(field, "invalid_characters"));
    }
    Ok(t.to_owned())
}

/// Basic, configurable heuristics. Returns the violation code, or `None` when the text looks fine.
/// `duplicate` says the same author recently posted identical text.
pub fn spam_code(body: &str, s: &Spam, duplicate: bool) -> Option<&'static str> {
    let lower = body.to_lowercase();
    let links = lower.matches("http://").count() + lower.matches("https://").count() + lower.matches("www.").count();
    if links > s.max_links {
        return Some("too_many_links");
    }
    if s.blocked_terms.iter().any(|t| lower.contains(&t.trim().to_lowercase())) {
        return Some("blocked_term");
    }
    let (mut run, mut prev) = (0usize, None);
    for c in body.chars().filter(|c| !c.is_whitespace()) {
        run = if prev == Some(c) { run + 1 } else { 1 };
        if run > s.max_repeat_run {
            return Some("repeated_characters");
        }
        prev = Some(c);
    }
    duplicate.then_some("duplicate_content")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactionKind {
    Like,
    Moving,
    Insightful,
}

impl ReactionKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "like" => Some(Self::Like),
            "moving" => Some(Self::Moving),
            "insightful" => Some(Self::Insightful),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Like => "like",
            Self::Moving => "moving",
            Self::Insightful => "insightful",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spam() -> Spam {
        Spam {
            max_links: 1,
            max_repeat_run: 5,
            duplicate_window_secs: 60,
            blocked_terms: vec!["Casino".into()],
        }
    }

    #[test]
    fn heuristics_flag_links_terms_runs_and_duplicates() {
        let s = spam();
        assert_eq!(spam_code("lovely, thank you", &s, false), None);
        assert_eq!(
            spam_code("see http://a.example and https://b.example", &s, false),
            Some("too_many_links")
        );
        assert_eq!(spam_code("best CASINO ever", &s, false), Some("blocked_term"));
        assert_eq!(spam_code("aaaaaaaa", &s, false), Some("repeated_characters"));
        assert_eq!(spam_code("lovely", &s, true), Some("duplicate_content"));
        // Letters of any script count as characters; combining text is not penalised.
        assert_eq!(spam_code("ما شاء الله جميل", &s, false), None);
    }

    #[test]
    fn text_counts_characters_not_bytes() {
        assert!(text("body", "مرحبا", 5).is_ok());
        assert_eq!(text("body", "مرحبا!", 5), Err(DomainError::invalid("body", "too_long")));
        assert_eq!(text("body", "  ", 5), Err(DomainError::invalid("body", "required")));
    }

    #[test]
    fn levels_must_ascend_and_pick_the_highest_reached() {
        let mut c: CommunitySettings = serde_json::from_value(serde_json::json!({
            "points": {}, "min_reputation": {}, "levels": [{"name":"a","min":0},{"name":"b","min":10}],
            "limits": {"comment_chars":10,"bio_chars":10,"report_detail_chars":10,"appeal_chars":10,"note_chars":10},
            "rate": {}, "spam": {"max_links":1,"max_repeat_run":5,"duplicate_window_secs":1,"blocked_terms":[]}
        }))
        .unwrap();
        assert!(c.validate().is_ok());
        assert_eq!(c.level_for(9), "a");
        assert_eq!(c.level_for(10), "b");
        c.levels.reverse();
        assert!(c.validate().is_err());
    }
}
