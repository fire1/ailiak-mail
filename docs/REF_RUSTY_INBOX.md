# Reference: Rusty Inbox

Peer / inspiration project for AiliakMail. Snapshot for local reference — upstream may change.

| | |
|---|---|
| **Repo** | https://github.com/karem505/rusty-inbox |
| **Site** | https://aboelmakarem.pro/apps/rusty-inbox |
| **Build story** | https://aboelmakarem.pro/blog/rusty-inbox-open-source-email-client-rust-qt |
| **License** | MIT OR Apache-2.0 |
| **Latest release (checked)** | **v0.2.2** (2026-11-13) |
| **Stars / forks** | ~27 / 2 (at capture time) |
| **Primary stack** | **Rust** mail engine + **Qt 6 / QML** UI (`cxx-qt`) |

## Local source code reference

Check for local existence inside of `~/Projects/GitHub/rusty-inbox`  

## Elevator pitch

Fast, native desktop email client (Linux / Windows / macOS): Rust IMAP/SMTP engine, Qt Quick UI (not Electron), Smart Inbox, keyboard-first, optional AI assistant. Omarchy-aware theming on Linux.

## Architecture (from upstream README)

```
app/                      desktop app: Qt 6 / QML via cxx-qt
  qml/                    inbox, reader, composer, settings
  src/bridge.rs           QObject bridge (JSON in, signals out)
crates/rusty-inbox-core/  mail engine (no UI)
  mail/                   IMAP (async-imap + rustls), SMTP (lettre), MIME
  store.rs                SQLite: threads, messages, outbox, settings
  compose.rs              drafts, reply/forward, mailto
  thunderbird.rs          Thunderbird import (NSS key4.db)
crates/rusty-inbox-ai/    AI assistant (no UI): providers, models, OAuth PKCE
```

**Requirements to build:** Rust 1.85+, Qt 6.8+ (Quick, Controls 2, **WebEngine**), C++ toolchain.

## Features worth noting vs AiliakMail

| Area | Rusty Inbox | Notes for us |
|------|-------------|--------------|
| UI toolkit | Qt 6 / QML + Qt WebEngine | We use egui + wry (WebKitGTK / WKWebView / WebView2) |
| Smart Inbox | People / notifications / newsletters groups | Similar split to our People + Staks |
| Sync | IMAP IDLE + poll fallback | We have POP3-first ABV path + IMAP |
| Compose | Rich text, signatures, Undo Send, Send Later | Local outbox / scheduled |
| People | Person card from From/To/Cc/Bcc | Overlaps our People chat idea |
| AI (0.2.0+) | ChatGPT OAuth, OpenAI, Anthropic, OpenRouter, Ollama | Proofread, rewrite, summarize, phishing check, Needs reply, etc. |
| Desktop | Tray, notifications, mailto, single instance | Omarchy theme live-follow |
| Import | Thunderbird accounts + passwords | Useful UX target |
| Updates | In-app update check + install | Flatpak / AppImage / NSIS / DMG aware |
| Privacy | Local SQLite; no telemetry | Same direction as us |



Windows: PowerShell install script or `.exe` / portable zip from Releases.  
macOS: `.dmg` (arm64 + x86_64).

## Release package sizes (v0.2.2)

Qt WebEngine is bundled → installers are much larger than our egui+wry builds (system webview).

| Asset | Size |
|-------|------|
| Linux x86_64 AppImage | ~150 MB |
| Linux aarch64 AppImage | ~157 MB |
| Linux x86_64 Flatpak | ~96 MB |
| Linux aarch64 Flatpak | ~86 MB |
| Linux tarball (x86_64 / aarch64) | ~9 MB each |
| Windows x64 setup.exe | ~88 MB |
| Windows x64 portable zip | ~126 MB |
| macOS arm64 / x86_64 DMG | ~269 MB each |

## Topics (GitHub)

`rust`, `qt6`, `qml`, `cxx-qt`, `email`, `imap`, `gmail`, `desktop-app`, `omarchy`, `ai`, `ollama`, `flatpak`, `thunderbird-alternative`

## Relevance to AiliakMail

- Closest open-source **native Rust** mail client on Linux/Omarchy with a polished shipping story.
- Useful reference for: Smart Inbox grouping, People surface, keyboard map, Thunderbird import, in-app updates, Flatpak/AUR packaging.
- **Not** a drop-in stack match: they chose Qt/QML + WebEngine; we chose egui + wry. Expect different binary size and HTML-mail embedding tradeoffs.
- Optional AI crate (`rusty-inbox-ai`) is a concrete checklist if we add assistant features later (Ailiak Desk bridge is our differentiator today).

### Adopted for HTML body rendering

Ported from rusty-inbox into `mailbox-core` (kept egui + wry, not Qt):

- Real `text/html` only (drop mail-parser synthesized HTML for plain-only mail).
- `cid:` → `data:` inline image rewrite at parse time (`mime_inline`).
- Webview document = light “paper”, keep author CSS, CSP + strip scripts (not nested `srcdoc` iframes — those paint blank under WebKitGTK/wry; rusty uses Chromium where iframe sandbox works).
- Privacy default differs: we keep remote images blocked; rusty-inbox loads them.

## Capture

- Captured from GitHub API + README / release v0.2.2.
- Date: 2026-11-24.
