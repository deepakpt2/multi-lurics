//! Catalogue value objects. Strong identifiers are normalized here, so storage and the
//! duplicate guard only ever see one spelling of each.

use crate::DomainError;

fn clean_text(field: &'static str, raw: &str, max_chars: usize) -> Result<String, DomainError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(DomainError::invalid(field, "required"));
    }
    if s.chars().count() > max_chars {
        return Err(DomainError::invalid(field, "too_long"));
    }
    if s.chars().any(char::is_control) {
        return Err(DomainError::invalid(field, "invalid_characters"));
    }
    Ok(s.to_owned())
}

/// Title or name as written, in any script. Stored verbatim; never translated.
pub fn title(field: &'static str, raw: &str) -> Result<String, DomainError> {
    clean_text(field, raw, 300)
}

/// Optional short note (artist disambiguation). Blank means absent.
pub fn optional_note(field: &'static str, raw: Option<&str>) -> Result<Option<String>, DomainError> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => clean_text(field, s, 200).map(Some),
    }
}

fn compact(raw: &str) -> String {
    raw.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.'))
        .flat_map(char::to_uppercase)
        .collect()
}

fn digit(c: char) -> u32 {
    c.to_digit(10).unwrap_or(0)
}

/// Parses an optional identifier; blank means absent.
fn optional_id(
    field: &'static str,
    raw: Option<&str>,
    parse: impl Fn(&str) -> Option<String>,
) -> Result<Option<String>, DomainError> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => parse(s)
            .map(Some)
            .ok_or_else(|| DomainError::invalid(field, "invalid_identifier")),
    }
}

/// ISWC `T-034.524.680-1` → `T0345246801`, check digit verified.
pub fn iswc(field: &'static str, raw: Option<&str>) -> Result<Option<String>, DomainError> {
    optional_id(field, raw, |s| {
        let c = compact(s);
        let body = c.strip_prefix('T')?;
        if body.len() != 10 || !body.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        let sum: u32 = 1 + body
            .chars()
            .take(9)
            .enumerate()
            .map(|(i, ch)| digit(ch) * (i as u32 + 1))
            .sum::<u32>();
        ((10 - sum % 10) % 10 == digit(body.chars().last()?)).then_some(c)
    })
}

/// ISRC `US-RC1-76-07839` → `USRC17607839`.
pub fn isrc(field: &'static str, raw: Option<&str>) -> Result<Option<String>, DomainError> {
    optional_id(field, raw, |s| {
        let c = compact(s);
        let b = c.as_bytes();
        let ok = b.len() == 12
            && b[..2].iter().all(u8::is_ascii_uppercase)
            && b[2..5].iter().all(|x| x.is_ascii_uppercase() || x.is_ascii_digit())
            && b[5..].iter().all(u8::is_ascii_digit);
        ok.then_some(c)
    })
}

/// ISNI `0000 0001 2103 2683` → `0000000121032683`, ISO 7064 Mod 11-2 verified.
pub fn isni(field: &'static str, raw: Option<&str>) -> Result<Option<String>, DomainError> {
    optional_id(field, raw, |s| {
        let c = compact(s);
        if c.len() != 16 || !c[..15].chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        let total = c[..15].chars().fold(0u32, |t, ch| ((t + digit(ch)) * 2) % 11);
        let check = match (12 - total) % 11 {
            10 => 'X',
            d => char::from_digit(d, 10)?,
        };
        c.ends_with(check).then_some(c)
    })
}

/// GTIN-8/12/13/14 barcode (UPC/EAN), digits only, GS1 check digit verified.
pub fn barcode(field: &'static str, raw: Option<&str>) -> Result<Option<String>, DomainError> {
    optional_id(field, raw, |s| {
        let c = compact(s);
        if !matches!(c.len(), 8 | 12 | 13 | 14) || !c.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        let digits: Vec<u32> = c.chars().map(digit).collect();
        let (check, body) = digits.split_last()?;
        let sum: u32 = body
            .iter()
            .rev()
            .enumerate()
            .map(|(i, d)| if i % 2 == 0 { d * 3 } else { *d })
            .sum();
        ((10 - sum % 10) % 10 == *check).then_some(c)
    })
}

pub fn year(field: &'static str, raw: Option<i32>) -> Result<Option<i16>, DomainError> {
    match raw {
        None => Ok(None),
        Some(y @ 1000..=2100) => Ok(Some(y as i16)),
        Some(_) => Err(DomainError::invalid(field, "out_of_range")),
    }
}

pub fn duration_ms(field: &'static str, raw: Option<i64>) -> Result<Option<i32>, DomainError> {
    match raw {
        None => Ok(None),
        Some(ms @ 1..=86_399_999) => Ok(Some(ms as i32)),
        Some(_) => Err(DomainError::invalid(field, "out_of_range")),
    }
}

macro_rules! closed_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }

            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

closed_enum!(ArtistKind { Person => "person", Group => "group", Other => "other" });

closed_enum!(AlbumKind {
    Album => "album", Single => "single", Ep => "ep",
    Compilation => "compilation", Soundtrack => "soundtrack", Other => "other",
});

closed_enum!(
    /// A single, unambiguous contribution. "Composer and lyricist" is two credits, never one string.
    CreditRole {
        Composer => "composer", Lyricist => "lyricist",
        Performer => "performer", FeaturedPerformer => "featured_performer",
        Arranger => "arranger", Producer => "producer", Conductor => "conductor",
        PrimaryArtist => "primary_artist", Translator => "translator", Adapter => "adapter",
    }
);

closed_enum!(
    RelationshipKind {
        Cover => "cover", Remake => "remake", Live => "live",
        AlternateArrangement => "alternate_arrangement", RelatedPerformance => "related_performance",
    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditTarget {
    Composition,
    Recording,
    Album,
}

impl CreditRole {
    /// Work-level roles belong to the composition, performance roles to the recording.
    pub fn applies_to(self, target: CreditTarget) -> bool {
        use CreditRole as R;
        match target {
            CreditTarget::Composition => matches!(self, R::Composer | R::Lyricist | R::Translator | R::Adapter),
            CreditTarget::Recording => matches!(
                self,
                R::Performer | R::FeaturedPerformer | R::Arranger | R::Producer | R::Conductor
            ),
            CreditTarget::Album => matches!(self, R::PrimaryArtist | R::Producer),
        }
    }
}

impl RelationshipKind {
    /// Every kind except a loosely related performance is another recording of the same work.
    pub fn requires_same_composition(self) -> bool {
        self != Self::RelatedPerformance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(e: DomainError) -> &'static str {
        let DomainError::Invalid(v) = e;
        v[0].code
    }

    #[test]
    fn identifiers_normalize_and_verify_check_digits() {
        assert_eq!(
            iswc("iswc", Some(" t-034.524.680-1 ")).unwrap().as_deref(),
            Some("T0345246801")
        );
        assert_eq!(
            code(iswc("iswc", Some("T-034.524.680-2")).unwrap_err()),
            "invalid_identifier"
        );
        assert_eq!(
            isrc("isrc", Some("us-rc1-76-07839")).unwrap().as_deref(),
            Some("USRC17607839")
        );
        assert_eq!(
            code(isrc("isrc", Some("USRC1760783")).unwrap_err()),
            "invalid_identifier"
        );
        assert_eq!(
            barcode("barcode", Some("4006381333931")).unwrap().as_deref(),
            Some("4006381333931")
        );
        assert_eq!(
            barcode("barcode", Some("036000 291452")).unwrap().as_deref(),
            Some("036000291452")
        );
        assert_eq!(
            code(barcode("barcode", Some("4006381333932")).unwrap_err()),
            "invalid_identifier"
        );
        assert_eq!(
            isni("isni", Some("0000 0001 2103 2683")).unwrap().as_deref(),
            Some("0000000121032683")
        );
        assert_eq!(
            isni("isni", Some("0000-0000-0000-001x")).unwrap().as_deref(),
            Some("000000000000001X")
        );
        assert_eq!(
            code(isni("isni", Some("0000000121032684")).unwrap_err()),
            "invalid_identifier"
        );
        assert_eq!(iswc("iswc", Some("  ")).unwrap(), None);
    }

    #[test]
    fn titles_keep_script_and_reject_blank() {
        assert_eq!(title("title", "  ராஜா ராஜாதி ").unwrap(), "ராஜா ராஜாதி");
        assert_eq!(code(title("title", " ").unwrap_err()), "required");
        assert_eq!(code(title("title", &"x".repeat(301)).unwrap_err()), "too_long");
    }

    #[test]
    fn credit_roles_are_scoped_to_their_entity() {
        assert!(CreditRole::Composer.applies_to(CreditTarget::Composition));
        assert!(!CreditRole::Composer.applies_to(CreditTarget::Recording));
        assert!(!CreditRole::Performer.applies_to(CreditTarget::Composition));
        assert!(CreditRole::Producer.applies_to(CreditTarget::Album));
        assert_eq!(CreditRole::parse("author"), None);
        assert!(CreditRole::ALL
            .iter()
            .all(|r| CreditRole::parse(r.as_str()) == Some(*r)));
        assert!(!RelationshipKind::RelatedPerformance.requires_same_composition());
        assert!(RelationshipKind::Cover.requires_same_composition());
    }
}
