//! Slug policy: turns a display title into a readable, stable URL segment. Pure and configurable;
//! the policy lives in the `seo` setting so an operator can change it without a deploy.

use serde::{Deserialize, Serialize};
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SlugPolicy {
    /// Keep letters of any script as written (NFC). When false (the default), addresses are Latin only:
    /// they are made from the English title, and only Latin text yields a slug.
    pub preserve_native_script: bool,
    /// Fold Latin letters with diacritics to their base letter (é → e). Other scripts are untouched.
    pub ascii_fold_latin: bool,
    pub max_length: usize,
    pub separator: char,
    /// A slug equal to one of these is never used as is: it would shadow a route or a word we own.
    pub reserved: Vec<String>,
}

impl Default for SlugPolicy {
    fn default() -> Self {
        Self {
            preserve_native_script: false,
            ascii_fold_latin: true,
            max_length: 80,
            separator: '-',
            reserved: [
                "new",
                "edit",
                "admin",
                "api",
                "search",
                "sitemap",
                "robots",
                "login",
                "register",
                "account",
                "moderation",
                "contributions",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

/// Marks that carry no meaning for a reader or are optional spelling aids: they must not change a URL.
fn ignorable(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}' | '\u{00AD}'
        | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}')
}

fn is_latin_base(c: char) -> bool {
    c.is_ascii_alphabetic()
}

impl SlugPolicy {
    /// `None` when nothing usable is left (punctuation only, or a script the policy does not keep);
    /// the caller then falls back to an id-based slug.
    pub fn slugify(&self, title: &str) -> Option<String> {
        let mut out = String::new();
        let mut pending_sep = false;
        for c in title.nfc().flat_map(|c| c.to_lowercase()) {
            if ignorable(c) {
                continue;
            }
            let keep = if self.ascii_fold_latin {
                // Fold only when the decomposition starts with a plain Latin letter.
                let mut d = std::iter::once(c).nfd();
                match d.next() {
                    Some(base) if is_latin_base(base) => Some(base),
                    _ => None,
                }
            } else {
                None
            };
            let c = keep.unwrap_or(c);
            let letter = c.is_alphanumeric() || (is_combining_mark(c) && !out.is_empty() && !pending_sep);
            if letter {
                if !self.preserve_native_script && !c.is_ascii() && !is_combining_mark(c) {
                    return None;
                }
                if pending_sep && !out.is_empty() {
                    out.push(self.separator);
                }
                pending_sep = false;
                out.push(c);
            } else {
                pending_sep = true;
            }
        }
        // Truncate on a character boundary, never leaving a dangling mark or separator.
        let mut slug: String = out.chars().take(self.max_length).collect();
        while slug.ends_with(self.separator) {
            slug.pop();
        }
        let slug = slug.nfc().collect::<String>();
        (!slug.is_empty()).then_some(slug)
    }

    pub fn is_reserved(&self, slug: &str) -> bool {
        self.reserved.iter().any(|r| r == slug)
    }
}

/// How a requested slug is compared with stored ones: percent-decoding is the caller's job; this
/// makes composed/decomposed and upper/lower-case spellings the same slug.
pub fn normalize_incoming(slug: &str) -> String {
    slug.nfc()
        .flat_map(|c| c.to_lowercase())
        .collect::<String>()
        .nfc()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native-script addresses, the operator's opt-in.
    fn native() -> SlugPolicy {
        SlugPolicy {
            preserve_native_script: true,
            ..SlugPolicy::default()
        }
    }

    fn s(t: &str) -> Option<String> {
        native().slugify(t)
    }

    #[test]
    fn readable_lowercase_with_collapsed_separators() {
        assert_eq!(s("Kaatrin Mozhi").as_deref(), Some("kaatrin-mozhi"));
        assert_eq!(s("  A. R.   Rahman -- (Live)! ").as_deref(), Some("a-r-rahman-live"));
        assert_eq!(s("Don't Stop").as_deref(), Some("don-t-stop"));
    }

    #[test]
    fn latin_diacritics_fold_but_native_scripts_survive_intact() {
        assert_eq!(s("Café Déjà Vu").as_deref(), Some("cafe-deja-vu"));
        // Malayalam: the virama and vowel signs stay, so the word is not mangled.
        assert_eq!(s("കാറ്റിൻ മൊഴി").as_deref(), Some("കാറ്റിൻ-മൊഴി"));
        // Arabic: letters stay, optional vowel marks and tatweel do not change the URL.
        assert_eq!(s("فَيْرُوز").as_deref(), s("فيروز").as_deref());
        assert_eq!(s("فيروز").as_deref(), Some("فيروز"));
        // Tamil and Devanagari combining signs are kept too.
        assert_eq!(s("இளையராஜா").as_deref(), Some("இளையராஜா"));
        assert_eq!(s("मेरा नाम").as_deref(), Some("मेरा-नाम"));
    }

    #[test]
    fn addresses_are_latin_only_by_default() {
        let p = SlugPolicy::default();
        assert_eq!(p.slugify("Vande Mataram").as_deref(), Some("vande-mataram"));
        assert_eq!(p.slugify("Café Déjà Vu").as_deref(), Some("cafe-deja-vu"));
        assert_eq!(p.slugify("വന്ദേ മാതരം"), None);
    }

    #[test]
    fn nfc_is_enforced_and_policy_is_configurable() {
        let composed = "caf\u{e9}";
        let decomposed = "cafe\u{301}";
        assert_eq!(s(composed), s(decomposed));
        let no_fold = SlugPolicy {
            ascii_fold_latin: false,
            ..native()
        };
        assert_eq!(no_fold.slugify(decomposed).as_deref(), Some("caf\u{e9}"));
        let ascii_only = SlugPolicy {
            preserve_native_script: false,
            ..Default::default()
        };
        assert_eq!(ascii_only.slugify("فيروز"), None);
        assert_eq!(ascii_only.slugify("Fairuz").as_deref(), Some("fairuz"));
        let short = SlugPolicy {
            max_length: 7,
            ..native()
        };
        assert_eq!(short.slugify("kaatrin mozhi").as_deref(), Some("kaatrin"));
        assert_eq!(short.slugify("ab cd efgh").as_deref(), Some("ab-cd-e"));
    }

    #[test]
    fn nothing_usable_is_none_and_reserved_words_are_flagged() {
        assert_eq!(s("!!! ... ???"), None);
        assert_eq!(s(""), None);
        let p = SlugPolicy::default();
        assert!(p.is_reserved(&s("Search").unwrap()));
        assert!(!p.is_reserved("search-party"));
    }

    #[test]
    fn incoming_slugs_are_normalized_the_same_way() {
        assert_eq!(normalize_incoming("Caf\u{e9}"), normalize_incoming("cafe\u{301}"));
        assert_eq!(normalize_incoming("KAATRIN-Mozhi"), "kaatrin-mozhi");
    }
}
