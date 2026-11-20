//! Production email delivery. The outbox row is the source of truth: a send that fails is retried
//! with backoff by the worker loop, and a redelivery after a crash may send a second copy, which is
//! harmless for verification and reset links (single use) and for notices.
//! Links are secrets: they are put in the message and never logged.

use crate::Handler;
use infrastructure::outbox::OutboxEvent;
use lettre::message::header::ContentType;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::{AsyncTransport, Message, Tokio1Executor};
use serde_json::Value;

pub struct Smtp {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: lettre::message::Mailbox,
}

impl Smtp {
    pub fn new(url: &str, from: &str) -> anyhow::Result<Self> {
        Ok(Self {
            transport: AsyncSmtpTransport::<Tokio1Executor>::from_url(url)?.build(),
            from: from.parse()?,
        })
    }
}

/// Subject and plain-text body for an email topic, in the user's locale (English fallback).
pub fn compose(topic: &str, payload: &Value) -> Option<(String, String)> {
    let ar = payload["locale"].as_str().is_some_and(|l| l.starts_with("ar"));
    let link = payload["link"].as_str().unwrap_or_default();
    let name = payload["display_name"].as_str().unwrap_or_default();
    let (en, ar_text): ((&str, String), (&str, String)) = match topic {
        "email.verify_requested" => (
            ("Confirm your email", format!("Hello {name},\n\nConfirm your email address to finish creating your account:\n{link}\n\nIf you did not sign up, ignore this message.")),
            ("تأكيد بريدك الإلكتروني", format!("مرحبًا {name}،\n\nأكّد بريدك الإلكتروني لإكمال إنشاء حسابك:\n{link}\n\nإذا لم تقم بالتسجيل فتجاهل هذه الرسالة.")),
        ),
        "email.password_reset_requested" => (
            ("Reset your password", format!("Use this link to choose a new password:\n{link}\n\nIf you did not ask for this, ignore this message; your password is unchanged.")),
            ("إعادة تعيين كلمة المرور", format!("استخدم هذا الرابط لاختيار كلمة مرور جديدة:\n{link}\n\nإذا لم تطلب ذلك فتجاهل هذه الرسالة؛ كلمة مرورك لم تتغير.")),
        ),
        "email.registration_attempted" => (
            ("Someone tried to register with your email", "An account already exists for this address. If it was you, sign in or reset your password. If not, you can ignore this message.".into()),
            ("محاولة تسجيل ببريدك الإلكتروني", "يوجد حساب بهذا البريد بالفعل. إذا كنت أنت، فسجّل الدخول أو أعد تعيين كلمة المرور. وإلا فتجاهل هذه الرسالة.".into()),
        ),
        "email.password_changed" => (
            ("Your password was changed", "The password for your account was just changed. If this was not you, reset it now and contact us.".into()),
            ("تم تغيير كلمة المرور", "تم تغيير كلمة مرور حسابك للتو. إذا لم تكن أنت، فأعد تعيينها الآن وتواصل معنا.".into()),
        ),
        _ => return None,
    };
    let (s, b) = if ar { ar_text } else { en };
    Some((s.to_owned(), b))
}

#[async_trait::async_trait]
impl Handler for Smtp {
    async fn handle(&self, e: &OutboxEvent) -> anyhow::Result<()> {
        // Content events have no mail consumer; the search/cache wrapper has already handled them.
        if e.topic.starts_with("lyrics.") {
            return Ok(());
        }
        let Some((subject, body)) = compose(&e.topic, &e.payload) else {
            anyhow::bail!("no mail template for topic '{}'", e.topic);
        };
        let to = e.payload["to"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("email event without recipient"))?;
        let msg = Message::builder()
            .from(self.from.clone())
            .to(to.parse()?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)?;
        self.transport.send(msg).await?;
        tracing::info!(id = %e.id, topic = %e.topic, "email sent");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn templates_use_the_users_locale_and_carry_the_link() {
        let p = json!({ "locale": "ar", "link": "https://x/ar/verify#token=abc", "display_name": "ليلى" });
        let (subject, body) = compose("email.verify_requested", &p).unwrap();
        assert_eq!(subject, "تأكيد بريدك الإلكتروني");
        assert!(body.contains("https://x/ar/verify#token=abc") && body.contains("ليلى"));
        let (subject, _) = compose(
            "email.password_reset_requested",
            &json!({ "locale": "en", "link": "l" }),
        )
        .unwrap();
        assert_eq!(subject, "Reset your password");
        assert!(compose("lyrics.published", &p).is_none());
    }
}
