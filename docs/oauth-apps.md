# AiliakMail OAuth apps (maintainer)

End users should only click **Sign in with Google / Microsoft / Yahoo / Yandex**.  
They must not create Cloud Console projects. That requires **one-time** registration of AiliakMail as an OAuth application for each provider, then embedding the Client IDs in the build.

## Where to put credentials

**Do not commit Client IDs or secrets** — GitHub push protection will reject the push, and Desktop client secrets are still sensitive.

1. **Local / preferred** — copy [`.env.example`](../.env.example) → `.env` (gitignored) and fill values. `build.rs` and the GUI load them automatically.

2. **Releases / CI** — set the same `AILIAKMAIL_OAUTH_*` vars as repository secrets, then:

   ```bash
   export AILIAKMAIL_OAUTH_GOOGLE_CLIENT_ID='….apps.googleusercontent.com'
   export AILIAKMAIL_OAUTH_GOOGLE_CLIENT_SECRET='…'
   export AILIAKMAIL_OAUTH_MICROSOFT_CLIENT_ID='xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx'
   export AILIAKMAIL_OAUTH_YAHOO_CLIENT_ID='…'
   export AILIAKMAIL_OAUTH_YAHOO_CLIENT_SECRET='…'
   export AILIAKMAIL_OAUTH_YANDEX_CLIENT_ID='…'
   export AILIAKMAIL_OAUTH_YANDEX_CLIENT_SECRET='…'
   cargo build -p mailbox-gui --release
   ```

3. **Overrides**: `[oauth]` in a **local** config.toml (not committed), or the env vars above.

## Register each provider

Redirect URI for all of these: loopback `http://127.0.0.1` (AiliakMail binds a random port; Google/Microsoft accept any port on 127.0.0.1 for desktop/public clients).

| Provider | Console | App type | Notes |
|----------|---------|----------|--------|
| Google | [Google Cloud Credentials](https://console.cloud.google.com/apis/credentials) | Desktop app | Scope `https://mail.google.com/` + openid email profile. Publish or add test users. |
| Microsoft | [Azure App registrations](https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps) | Public client / native | Redirect `http://127.0.0.1`. Permissions: `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access`, `openid`, `email`, `profile`. |
| Yahoo | [Yahoo Developer](https://developer.yahoo.com/apps/) | Web / installed | Needs Client ID **and** Secret. Scope `openid mail-w`. |
| Yandex | [Yandex OAuth](https://oauth.yandex.com/) | Application | Needs Client ID **and** Secret. Scopes `mail:imap_full`, `mail:smtp`, `login:email`. |

## Providers without IMAP OAuth

- **iCloud** — Apple app-specific password only  
- **GMX** — account password  
- **Custom IMAP/POP3** — password / app password  

## Consent screen branding

After registration, the browser should say **AiliakMail** wants access to mail — that is app authorization, not typing a mail password into AiliakMail.
