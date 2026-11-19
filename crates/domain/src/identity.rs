use crate::DomainError;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Email(String);

impl Email {
    pub const FIELD: &'static str = "email";

    /// Shape check only; ownership is proven by the verification flow.
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let s = raw.trim();
        let invalid = || DomainError::invalid(Self::FIELD, "invalid_email");
        if s.is_empty() {
            return Err(DomainError::invalid(Self::FIELD, "required"));
        }
        if s.len() > 254 || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(invalid());
        }
        let (local, host) = s.split_once('@').ok_or_else(invalid)?;
        if local.is_empty() || host.contains('@') || !host.contains('.') || host.starts_with('.') || host.ends_with('.')
        {
            return Err(invalid());
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A person's chosen name. Stored verbatim and never machine-translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayName(String);

impl DisplayName {
    pub const FIELD: &'static str = "display_name";
    pub const MAX_CHARS: usize = 64;

    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let s = raw.trim();
        if s.is_empty() {
            return Err(DomainError::invalid(Self::FIELD, "required"));
        }
        if s.chars().count() > Self::MAX_CHARS {
            return Err(DomainError::invalid(Self::FIELD, "too_long"));
        }
        if s.chars().any(char::is_control) {
            return Err(DomainError::invalid(Self::FIELD, "invalid_characters"));
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A plaintext password that satisfies policy. Not `Debug`-printable.
pub struct Password(String);

impl Password {
    pub const MIN_CHARS: usize = 12;
    pub const MAX_CHARS: usize = 128;

    pub fn parse(field: &'static str, raw: &str) -> Result<Self, DomainError> {
        let n = raw.chars().count();
        if n == 0 {
            return Err(DomainError::invalid(field, "required"));
        }
        if n < Self::MIN_CHARS {
            return Err(DomainError::invalid(field, "too_short"));
        }
        if n > Self::MAX_CHARS {
            return Err(DomainError::invalid(field, "too_long"));
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(***)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Moderator,
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Moderator => "moderator",
            Self::Admin => "admin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "moderator" => Some(Self::Moderator),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Unverified,
    Active,
    Disabled,
}

impl UserStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unverified => "unverified",
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unverified" => Some(Self::Unverified),
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }

    pub fn can_sign_in(self) -> bool {
        self == Self::Active
    }
}

/// The authenticated actor, derived only from a server-side session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub roles: Vec<Role>,
}

impl Principal {
    /// Admin implies every elevated role.
    pub fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role) || self.roles.contains(&Role::Admin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FieldViolation;

    fn code(e: DomainError) -> &'static str {
        let DomainError::Invalid(v) = e;
        v[0].code
    }

    #[test]
    fn email_accepts_and_trims() {
        assert_eq!(Email::parse("  a@b.co ").unwrap().as_str(), "a@b.co");
    }

    #[test]
    fn email_rejects_bad_shapes() {
        assert_eq!(code(Email::parse("").unwrap_err()), "required");
        for bad in ["nope", "a@b", "@b.co", "a@@b.co", "a b@c.co", "a@.co", "a@co."] {
            assert_eq!(code(Email::parse(bad).unwrap_err()), "invalid_email", "{bad}");
        }
    }

    #[test]
    fn display_name_keeps_script_and_limits_length() {
        assert_eq!(DisplayName::parse(" இளையராஜா ").unwrap().as_str(), "இளையராஜா");
        assert_eq!(DisplayName::parse("فيروز").unwrap().as_str(), "فيروز");
        assert_eq!(code(DisplayName::parse(&"x".repeat(65)).unwrap_err()), "too_long");
        assert_eq!(code(DisplayName::parse("a\u{0007}").unwrap_err()), "invalid_characters");
        assert_eq!(code(DisplayName::parse("  ").unwrap_err()), "required");
    }

    #[test]
    fn password_policy_counts_chars_not_bytes() {
        assert_eq!(code(Password::parse("password", "short").unwrap_err()), "too_short");
        assert!(Password::parse("password", &"ஆ".repeat(12)).is_ok());
        assert_eq!(
            code(Password::parse("password", &"x".repeat(129)).unwrap_err()),
            "too_long"
        );
        assert_eq!(
            Password::parse("new_password", "").unwrap_err(),
            DomainError::Invalid(vec![FieldViolation {
                field: "new_password",
                code: "required"
            }])
        );
    }

    #[test]
    fn password_debug_is_redacted() {
        let p = Password::parse("password", "correct horse battery").unwrap();
        assert_eq!(format!("{p:?}"), "Password(***)");
    }

    #[test]
    fn admin_implies_moderator_but_not_vice_versa() {
        let p = |roles| Principal {
            user_id: Uuid::nil(),
            session_id: Uuid::nil(),
            roles,
        };
        assert!(p(vec![Role::Admin]).has_role(Role::Moderator));
        assert!(!p(vec![Role::Moderator]).has_role(Role::Admin));
        assert!(!p(vec![]).has_role(Role::Moderator));
    }

    #[test]
    fn only_active_users_sign_in() {
        assert!(UserStatus::Active.can_sign_in());
        assert!(!UserStatus::Unverified.can_sign_in());
        assert!(!UserStatus::Disabled.can_sign_in());
    }
}
