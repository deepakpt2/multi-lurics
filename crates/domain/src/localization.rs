//! Localized metadata: value objects and the one display-fallback algorithm every read path uses.

use crate::DomainError;
use uuid::Uuid;

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

closed_enum!(
    /// Songwriters and translators are artists credited in those roles: one person, one id.
    EntityType {
        Composition => "composition", Recording => "recording", Artist => "artist", Album => "album",
        Label => "label", Publisher => "publisher", Genre => "genre", CreditRole => "credit_role",
    }
);

impl EntityType {
    /// Fixed table names (never user input), used to read `default_language_id`.
    pub fn table(self) -> &'static str {
        match self {
            Self::Composition => "compositions",
            Self::Recording => "recordings",
            Self::Artist => "artists",
            Self::Album => "albums",
            Self::Label => "labels",
            Self::Publisher => "publishers",
            Self::Genre => "genres",
            Self::CreditRole => "credit_roles",
        }
    }

    /// Personal names: never machine-translated, always sourced.
    pub fn is_personal_name(self) -> bool {
        self == Self::Artist
    }
}

closed_enum!(Provenance {
    Original => "original", HumanTranslation => "human_translation", Transliteration => "transliteration",
    Community => "community", Imported => "imported", Machine => "machine",
});

closed_enum!(VerificationState { Pending => "pending", Verified => "verified", Rejected => "rejected" });

closed_enum!(
    /// How a credit role is grouped on display. Original writers, translators and adapters never mix.
    RoleCategory {
        OriginalSongwriter => "original_songwriter", Translator => "translator", Adapter => "adapter",
        Performance => "performance", Production => "production", Other => "other",
    }
);

fn text(field: &'static str, raw: &str, max: usize) -> Result<String, DomainError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(DomainError::invalid(field, "required"));
    }
    if s.chars().count() > max {
        return Err(DomainError::invalid(field, "too_long"));
    }
    // Control characters (Cc) are refused; bidi formatting marks (Cf) are legitimate in
    // mixed-direction text and are stored exactly as given.
    if s.chars().any(char::is_control) {
        return Err(DomainError::invalid(field, "invalid_characters"));
    }
    Ok(s.to_owned())
}

pub fn name(raw: &str) -> Result<String, DomainError> {
    text("name", raw, 300)
}

pub fn alias(raw: &str) -> Result<String, DomainError> {
    text("alias", raw, 300)
}

pub fn description(raw: Option<&str>) -> Result<Option<String>, DomainError> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => text("description", s, 2000).map(Some),
    }
}

/// Where a name comes from. Mandatory for personal names, optional otherwise.
pub fn source(raw: Option<&str>, required: bool) -> Result<Option<String>, DomainError> {
    match raw.map(str::trim) {
        None | Some("") if required => Err(DomainError::invalid("source", "required")),
        None | Some("") => Ok(None),
        Some(s) => text("source", s, 500).map(Some),
    }
}

/// Machine output never stands in for a personal name.
pub fn check_provenance(entity: EntityType, p: Provenance) -> Result<(), DomainError> {
    if entity.is_personal_name() && p == Provenance::Machine {
        return Err(DomainError::invalid("provenance", "machine_not_allowed"));
    }
    Ok(())
}

/// A requested display locale such as `ml`, `ar-EG`, `sr-Latn`: language subtag plus optional script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleTag {
    pub language: String,
    pub script: Option<String>,
}

impl LocaleTag {
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let bad = || DomainError::invalid("lang", "invalid_language_tag");
        let mut parts = raw.trim().split(['-', '_']);
        let lang = parts.next().ok_or_else(bad)?;
        if !(2..=3).contains(&lang.len()) || !lang.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(bad());
        }
        let mut script = None;
        for p in parts {
            // Only a four-letter subtag is a script; region/variant subtags are ignored.
            if p.len() == 4 && p.chars().all(|c| c.is_ascii_alphabetic()) && script.is_none() {
                let mut cs = p.chars();
                let first = cs.next().map(|c| c.to_ascii_uppercase()).unwrap_or_default();
                script = Some(format!("{first}{}", cs.as_str().to_ascii_lowercase()));
            } else if p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(bad());
            }
        }
        Ok(Self {
            language: lang.to_ascii_lowercase(),
            script,
        })
    }
}

/// One verified localization of an entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub language_id: Uuid,
    pub script_id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub is_preferred: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requested {
    pub language_id: Option<Uuid>,
    pub script_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resolution {
    /// A localization in the requested language.
    Requested,
    /// None in the requested language; shown in the entity's own (original) language.
    DefaultLanguage,
    /// Neither; some other available language.
    Fallback,
    /// No localization at all; the base name stored on the entity.
    Base,
}

impl Resolution {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::DefaultLanguage => "default_language",
            Self::Fallback => "fallback",
            Self::Base => "base",
        }
    }

    /// True whenever the text is not in the language the reader asked for, so the UI can label it.
    pub fn is_fallback(self) -> bool {
        self != Self::Requested
    }
}

/// The single display resolver. `candidates` must be ordered preferred-first, then oldest-first;
/// within a language the requested script wins, then the preferred row.
///
/// Chain: requested language → entity default language → any available → base name.
/// The base name stored on an entity is written in its default language, so when a default
/// language is declared but has no localization row, the result is `(None, DefaultLanguage)`:
/// "use the base name, which is in the default language".
pub fn resolve<'a>(
    candidates: &'a [Candidate],
    req: &Requested,
    default_language: Option<Uuid>,
) -> (Option<&'a Candidate>, Resolution) {
    let in_language = |lang: Uuid| {
        let mut it = candidates.iter().filter(move |c| c.language_id == lang);
        let first = it.next()?;
        let scripted = req.script_id.and_then(|s| {
            candidates
                .iter()
                .find(|c| c.language_id == lang && c.script_id == Some(s))
        });
        Some(scripted.unwrap_or(first))
    };
    if let Some(c) = req.language_id.and_then(in_language) {
        return (Some(c), Resolution::Requested);
    }
    if let Some(lang) = default_language {
        return (in_language(lang), Resolution::DefaultLanguage);
    }
    match candidates.first() {
        Some(c) => (Some(c), Resolution::Fallback),
        None => (None, Resolution::Base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(lang: u128, script: Option<u128>, name: &str, preferred: bool) -> Candidate {
        Candidate {
            language_id: Uuid::from_u128(lang),
            script_id: script.map(Uuid::from_u128),
            name: name.into(),
            description: None,
            is_preferred: preferred,
        }
    }

    fn pick(
        c: &[Candidate],
        lang: Option<u128>,
        script: Option<u128>,
        default: Option<u128>,
    ) -> (Option<String>, Resolution) {
        let req = Requested {
            language_id: lang.map(Uuid::from_u128),
            script_id: script.map(Uuid::from_u128),
        };
        let (c, r) = resolve(c, &req, default.map(Uuid::from_u128));
        (c.map(|c| c.name.clone()), r)
    }

    #[test]
    fn chain_requested_then_default_then_any_then_base() {
        let c = [cand(1, None, "en-name", true), cand(2, None, "ml-name", true)];
        assert_eq!(
            pick(&c, Some(2), None, Some(1)),
            (Some("ml-name".into()), Resolution::Requested)
        );
        assert_eq!(
            pick(&c, Some(9), None, Some(2)),
            (Some("ml-name".into()), Resolution::DefaultLanguage)
        );
        // Default language declared but unlocalized: the base name (None) is in that language.
        assert_eq!(pick(&c, Some(9), None, Some(8)), (None, Resolution::DefaultLanguage));
        assert_eq!(
            pick(&c, Some(9), None, None),
            (Some("en-name".into()), Resolution::Fallback)
        );
        assert_eq!(
            pick(&c, None, None, None),
            (Some("en-name".into()), Resolution::Fallback)
        );
        assert_eq!(pick(&[], Some(1), None, Some(1)), (None, Resolution::DefaultLanguage));
        assert_eq!(pick(&[], Some(1), None, None), (None, Resolution::Base));
        assert!(Resolution::Base.is_fallback() && !Resolution::Requested.is_fallback());
    }

    #[test]
    fn requested_script_wins_within_a_language() {
        let c = [cand(7, Some(10), "latin", true), cand(7, Some(11), "cyrillic", false)];
        assert_eq!(pick(&c, Some(7), Some(11), None).0.as_deref(), Some("cyrillic"));
        assert_eq!(pick(&c, Some(7), None, None).0.as_deref(), Some("latin"));
        // Unknown script for the language: still the requested language, not a fallback.
        assert_eq!(
            pick(&c, Some(7), Some(99), None),
            (Some("latin".into()), Resolution::Requested)
        );
    }

    #[test]
    fn locale_tags_and_text_rules() {
        assert_eq!(
            LocaleTag::parse("ML").unwrap(),
            LocaleTag {
                language: "ml".into(),
                script: None
            }
        );
        assert_eq!(
            LocaleTag::parse("sr-latn-RS").unwrap(),
            LocaleTag {
                language: "sr".into(),
                script: Some("Latn".into())
            }
        );
        for bad in ["", "e", "english", "en-", "e1", "en-!!"] {
            assert!(LocaleTag::parse(bad).is_err(), "{bad}");
        }
        // Bidi marks survive; control characters do not.
        assert_eq!(name("  أغنية\u{200F} Song 2 ").unwrap(), "أغنية\u{200F} Song 2");
        assert!(name("a\u{0007}").is_err());
        assert!(source(None, true).is_err() && source(Some(" "), false).unwrap().is_none());
        assert!(check_provenance(EntityType::Artist, Provenance::Machine).is_err());
        assert!(check_provenance(EntityType::Genre, Provenance::Machine).is_ok());
    }
}
