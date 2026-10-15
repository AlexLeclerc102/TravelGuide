//! Envoi d'emails (liens magiques, invitations).

use std::sync::Mutex;

use anyhow::Context;
use async_trait::async_trait;
use lettre::message::Mailbox;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

#[derive(Debug, Clone)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub text: String,
}

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, email: Email) -> anyhow::Result<()>;
}

/// Envoi réel via SMTP (Mailpit en dev).
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    pub fn new(url: &str, from: &str) -> anyhow::Result<Self> {
        Ok(Self {
            transport: AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
                .context("SMTP_URL invalide")?
                .build(),
            from: from.parse().context("MAIL_FROM invalide")?,
        })
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, email: Email) -> anyhow::Result<()> {
        let message = Message::builder()
            .from(self.from.clone())
            .to(email.to.parse().context("destinataire invalide")?)
            .subject(email.subject)
            .body(email.text)?;
        self.transport.send(message).await?;
        Ok(())
    }
}

/// Sans SMTP configuré : l'email est écrit dans les journaux (développement uniquement).
pub struct LogMailer;

#[async_trait]
impl Mailer for LogMailer {
    async fn send(&self, email: Email) -> anyhow::Result<()> {
        tracing::warn!(to = %email.to, subject = %email.subject, body = %email.text,
            "SMTP_URL absent : email non envoyé, contenu journalisé (dev uniquement)");
        Ok(())
    }
}

/// Conserve les emails en mémoire (tests).
#[derive(Default)]
pub struct MemoryMailer {
    pub sent: Mutex<Vec<Email>>,
}

#[async_trait]
impl Mailer for MemoryMailer {
    async fn send(&self, email: Email) -> anyhow::Result<()> {
        self.sent.lock().expect("mutex empoisonné").push(email);
        Ok(())
    }
}
