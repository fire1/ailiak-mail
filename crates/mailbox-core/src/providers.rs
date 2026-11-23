//! Built-in mail provider presets for easy account setup.

use crate::config::MailProtocol;

/// Well-known providers + custom IMAP / POP3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MailProvider {
    Gmail,
    Yahoo,
    Office365,
    Outlook,
    Icloud,
    Yandex,
    Gmx,
    /// ABV.bg (Bulgaria) — POP3 only.
    Abv,
    /// Custom IMAP + SMTP (user fills hosts).
    Imap,
    /// Custom POP3 + SMTP (user fills hosts).
    Pop3,
}

/// Stored defaults applied when the user picks a provider.
#[derive(Debug, Clone, Copy)]
pub struct ProviderPreset {
    pub provider: MailProvider,
    pub label: &'static str,
    pub blurb: &'static str,
    pub protocol: MailProtocol,
    pub imap_host: &'static str,
    pub imap_port: u16,
    pub imap_tls: bool,
    pub smtp_host: &'static str,
    pub smtp_port: u16,
    pub smtp_tls: bool,
    /// Shown under the password field (app passwords, etc.).
    pub password_hint: &'static str,
    /// Browser OAuth is available for this provider.
    pub supports_oauth: bool,
    /// Primary CTA label when OAuth is available.
    pub oauth_button: &'static str,
}

impl MailProvider {
    pub const ALL: [Self; 10] = [
        Self::Gmail,
        Self::Yahoo,
        Self::Office365,
        Self::Outlook,
        Self::Icloud,
        Self::Yandex,
        Self::Gmx,
        Self::Abv,
        Self::Imap,
        Self::Pop3,
    ];

    pub fn preset(self) -> ProviderPreset {
        match self {
            Self::Gmail => ProviderPreset {
                provider: self,
                label: "Gmail",
                blurb: "Google Mail",
                protocol: MailProtocol::Imap,
                imap_host: "imap.gmail.com",
                imap_port: 993,
                imap_tls: true,
                smtp_host: "smtp.gmail.com",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "App password fallback — normally use Sign in with Google.",
                supports_oauth: true,
                oauth_button: "Authorize with Google",
            },
            Self::Yahoo => ProviderPreset {
                provider: self,
                label: "Yahoo",
                blurb: "Yahoo Mail",
                protocol: MailProtocol::Imap,
                imap_host: "imap.mail.yahoo.com",
                imap_port: 993,
                imap_tls: true,
                smtp_host: "smtp.mail.yahoo.com",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "App password fallback — normally use Sign in with Yahoo.",
                supports_oauth: true,
                oauth_button: "Authorize with Yahoo",
            },
            Self::Office365 => ProviderPreset {
                provider: self,
                label: "Office 365",
                blurb: "Microsoft 365 work / school",
                protocol: MailProtocol::Imap,
                imap_host: "outlook.office365.com",
                imap_port: 993,
                imap_tls: true,
                smtp_host: "smtp.office365.com",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "Password fallback — many orgs require Authorize with Microsoft.",
                supports_oauth: true,
                oauth_button: "Authorize with Microsoft",
            },
            Self::Outlook => ProviderPreset {
                provider: self,
                label: "Outlook",
                blurb: "Outlook.com / Hotmail / Live",
                protocol: MailProtocol::Imap,
                imap_host: "outlook.office365.com",
                imap_port: 993,
                imap_tls: true,
                smtp_host: "smtp.office365.com",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "App password fallback — normally use Authorize with Microsoft.",
                supports_oauth: true,
                oauth_button: "Authorize with Microsoft",
            },
            Self::Icloud => ProviderPreset {
                provider: self,
                label: "iCloud",
                blurb: "Apple iCloud Mail",
                protocol: MailProtocol::Imap,
                imap_host: "imap.mail.me.com",
                imap_port: 993,
                imap_tls: true,
                smtp_host: "smtp.mail.me.com",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "Apple requires an app-specific password (appleid.apple.com → Sign-In and Security).",
                supports_oauth: false,
                oauth_button: "",
            },
            Self::Yandex => ProviderPreset {
                provider: self,
                label: "Yandex",
                blurb: "Yandex Mail",
                protocol: MailProtocol::Imap,
                imap_host: "imap.yandex.com",
                imap_port: 993,
                imap_tls: true,
                smtp_host: "smtp.yandex.com",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "App password fallback — normally use Authorize with Yandex.",
                supports_oauth: true,
                oauth_button: "Authorize with Yandex",
            },
            Self::Gmx => ProviderPreset {
                provider: self,
                label: "GMX",
                blurb: "GMX Mail",
                protocol: MailProtocol::Imap,
                imap_host: "imap.gmx.com",
                imap_port: 993,
                imap_tls: true,
                smtp_host: "mail.gmx.com",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "GMX account password (browser OAuth is not available for GMX).",
                supports_oauth: false,
                oauth_button: "",
            },
            Self::Abv => ProviderPreset {
                provider: self,
                label: "ABV",
                blurb: "abv.bg — POP3 Inbox only (no IMAP folders)",
                protocol: MailProtocol::Pop3,
                imap_host: "pop3.abv.bg",
                imap_port: 995,
                imap_tls: true,
                smtp_host: "smtp.abv.bg",
                smtp_port: 465,
                smtp_tls: true,
                password_hint: "Full address (you@abv.bg) + mailbox password. Enable POP3 in ABV webmail settings if login fails. ABV has no IMAP — Sent/Spam/custom folders stay in webmail only.",
                supports_oauth: false,
                oauth_button: "",
            },
            Self::Imap => ProviderPreset {
                provider: self,
                label: "IMAP",
                blurb: "Custom IMAP + SMTP",
                protocol: MailProtocol::Imap,
                imap_host: "",
                imap_port: 993,
                imap_tls: true,
                smtp_host: "",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "Mail password or app password",
                supports_oauth: false,
                oauth_button: "",
            },
            Self::Pop3 => ProviderPreset {
                provider: self,
                label: "POP3",
                blurb: "Custom POP3 + SMTP",
                protocol: MailProtocol::Pop3,
                imap_host: "",
                imap_port: 995,
                imap_tls: true,
                smtp_host: "",
                smtp_port: 587,
                smtp_tls: true,
                password_hint: "POP3 password",
                supports_oauth: false,
                oauth_button: "",
            },
        }
    }

    pub fn needs_custom_hosts(self) -> bool {
        matches!(self, Self::Imap | Self::Pop3)
    }

    pub fn supports_oauth(self) -> bool {
        self.preset().supports_oauth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_has_a_preset() {
        for p in MailProvider::ALL {
            let preset = p.preset();
            assert_eq!(preset.provider, p);
            assert!(!preset.label.is_empty());
            if !p.needs_custom_hosts() {
                assert!(!preset.imap_host.is_empty());
                assert!(!preset.smtp_host.is_empty());
            }
        }
    }
}
