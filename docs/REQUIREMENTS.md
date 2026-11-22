# AiliakMail — task requirements (Rust / Linux)

## 1. Vision

**Mailbox** is a calm, offline-capable Linux email client written in Rust.  
Primary UX: **terminal UI (TUI)**. Optional later: minimal GUI shell.

It is **not** a webmail clone inside Ailiak. It is a standalone MUA that can **hand selected mail to Ailiak as Desk tasks**.

One-liner: *Read and act on mail locally; send work that needs tracking to Ailiak.*

---

## 2. Goals / non-goals

### Goals (v1)

- [ ] Multi-account IMAP + SMTP (start with one account; schema supports many)
- [ ] Local message cache (SQLite and/or Maildir)
- [ ] TUI: folder list, envelope list, thread/read, compose/reply/forward
- [ ] Search (local index at least)
- [ ] Secrets in OS keyring (libsecret / secret-service), not plaintext config
- [ ] Optional **Ailiak send-to-desk** action (see `AILIAK_INTEGRATION.md`)

### Non-goals (v1)

- Full HTML fidelity / browser engine in the TUI
- Built-in calendar / contacts suite (vCard import later is OK)
- Replacing Thunderbird feature-for-feature
- Hosting user mail on Ailiak servers
- Embedding Mailbox inside the Ailiak web SPA

### Stretch (post-v1)

- JMAP, Gmail API, Microsoft Graph
- notmuch / Maildir-native workflows
- PGP (sign / encrypt / verify)
- GUI (egui / iced / Tauri) sharing the same core crate
- Background sync daemon + notifications

---

## 3. Users & platforms

| Item | Requirement |
|------|-------------|
| OS | Linux (x86_64 first; aarch64 nice-to-have) |
| Runtime | No Node/Electron; native Rust binary |
| Terminal | Truecolor optional; `NO_COLOR` respected |
| Distro packaging | Cargo install first; `.deb` / Flathub later |

---

## 4. Architecture (required shape)

```
mailbox/                 # workspace
  crates/
    mailbox-core/        # accounts, sync, store, MIME, search
    mailbox-tui/         # ratatui (or similar) frontend
    mailbox-ailiak/      # optional: Agent API / MCP client
  docs/
```

Rules:

1. **UI never talks IMAP directly** — only through `mailbox-core`.
2. **Sync is interruptible** — cancel-safe; UI stays responsive.
3. **Config is TOML** under `$XDG_CONFIG_HOME/mailbox/`.
4. **Cache/data** under `$XDG_DATA_HOME/mailbox/`.
5. **Ailiak is a plugin-like crate** — core builds without it if feature-gated.

Suggested stack (guidance, not locked):

| Layer | Candidates |
|-------|------------|
| Async | `tokio` |
| IMAP | `async-imap` / similar mature crate |
| SMTP | `lettre` |
| MIME | `mail-parser` / `mail-builder` |
| Store | `rusqlite` or `sqlx` + optional Maildir mirror |
| TUI | `ratatui` + `crossterm` |
| Secrets | `keyring` |
| Config | `serde` + `toml` |
| HTTP (Ailiak) | `reqwest` |

Reusing ideas/crates from [meli](https://meli-email.org/) / [Himalaya](https://github.com/pimalaya/himalaya) is encouraged; do not vendor their whole apps.

---

## 5. Functional requirements

### 5.1 Accounts

| ID | Requirement | Priority |
|----|-------------|----------|
| A1 | Add IMAP account: host, port, TLS, username; password via keyring | P0 |
| A2 | Add SMTP submission settings (may share host) | P0 |
| A3 | Test connection (IMAP login + SMTP auth) before save | P0 |
| A4 | Multiple named accounts; switch active account in UI | P1 |
| A5 | OAuth2 / XOAUTH2 (Gmail, Microsoft) | P2 |

### 5.2 Sync & storage

| ID | Requirement | Priority |
|----|-------------|----------|
| S1 | Fetch folder list; open INBOX by default | P0 |
| S2 | Incremental envelope sync (UID-based) | P0 |
| S3 | On-demand full message body + attachments metadata | P0 |
| S4 | Persist envelopes/bodies offline; open last sync without network | P0 |
| S5 | Mark seen / unseen; move/delete (IMAP flags + expunge or Trash) | P0 |
| S6 | Attachment save-to-disk | P1 |
| S7 | Background idle / periodic refresh | P1 |
| S8 | Full-text local search | P1 |

### 5.3 Reading & composing

| ID | Requirement | Priority |
|----|-------------|----------|
| R1 | Envelope list sorted by date; unread indicator | P0 |
| R2 | Open message: headers, plain-text body (HTML → text via filter cmd optional) | P0 |
| R3 | Thread grouping (by `Message-ID` / `In-Reply-To` / `References`) | P1 |
| R4 | Compose new / reply / reply-all / forward in `$EDITOR` or in-TUI buffer | P0 |
| R5 | Attach files on compose | P1 |
| R6 | Drafts mailbox | P2 |

### 5.4 UX (TUI)

| ID | Requirement | Priority |
|----|-------------|----------|
| U1 | Keyboard-first navigation (hjkl / arrows, documented cheatsheet) | P0 |
| U2 | Status line: account, folder, sync state, errors | P0 |
| U3 | Config command or first-run wizard | P0 |
| U4 | Theme via config (fg/bg/accent); respect `NO_COLOR` | P1 |
| U5 | Help overlay (`?`) | P0 |

### 5.5 Security & privacy

| ID | Requirement | Priority |
|----|-------------|----------|
| P1 | No password in git or world-readable config | P0 |
| P2 | TLS required by default; explicit opt-in for insecure | P0 |
| P3 | HTML sanitized / not executed; prefer plain text | P0 |
| P4 | Ailiak token stored in keyring separately from mail password | P0 |
| P5 | Clear logging policy: never log bodies or secrets | P0 |

### 5.6 Ailiak bridge (optional feature)

| ID | Requirement | Priority |
|----|-------------|----------|
| K1 | Configure Agent API base URL + `ailiak_…` token | P1 |
| K2 | Action: **Send to Ailiak** from a message / selection | P1 |
| K3 | Creates Desk task via MCP/`create_task` (title, detail, `embedUrl`) | P1 |
| K4 | Dedup: do not create duplicate task for same `Message-ID` / embed URL | P1 |
| K5 | Show result (task id / error) in status line | P1 |

Details: [`AILIAK_INTEGRATION.md`](AILIAK_INTEGRATION.md).

---

## 6. Phased delivery (implementation tasks)

### Phase 0 — Skeleton

- [x] Cargo workspace + `mailbox-core` / `mailbox-gui` crates (TUI deferred; GUI requested)
- [x] XDG config/data paths + sample `config.toml`
- [x] CI: `cargo fmt`, `clippy`, `test` on Linux
- [x] README: build & run

**Done when:** `cargo run -p mailbox-gui` opens the mail shell (demo cache).

### Phase 1 — One account, read path

- [x] Account config + keyring password (Gmail App Password flow in GUI)
- [x] IMAP connect, list folders, sync selected folder (default INBOX) to SQLite
- [x] GUI: folders + envelope list + open plain body
- [x] Offline reopen of cached messages

**Done when:** user can read INBOX mail offline after one sync.

### Phase 2 — Act on mail

- [ ] Flags: seen, delete/trash, move folder
- [ ] Compose / reply via editor + SMTP send
- [ ] Error surfacing + retry sync

**Done when:** user can reply to a message end-to-end.

### Phase 3 — Polish + Ailiak

- [ ] Search, threads, attachments save
- [ ] Multi-account switch
- [ ] `mailbox-ailiak` feature: send message → Desk task
- [ ] Man page / `mailbox --help` + in-app `?`

**Done when:** “Send to Ailiak” creates a task with subject + link/detail.

### Phase 4 — Harden

- [ ] IDLE / background refresh
- [ ] Packaging (cargo-dist or `.deb`)
- [ ] OAuth providers (if demanded)
- [ ] Optional GUI frontend sharing core

---

## 7. Acceptance criteria (v1 exit)

1. Fresh Linux user can configure one IMAP/SMTP account without editing secrets in plaintext.
2. After sync, INBOX is usable with network off.
3. Reply sends via SMTP and appears in Sent (or configured Sent folder) when server supports APPEND.
4. With Ailiak token configured, **Send to Ailiak** creates exactly one Desk task for a message (`embedUrl` or stable external id set).
5. No credentials in logs or crash reports.
6. `cargo test` and clippy clean on CI.

---

## 8. Open questions

1. Default body strategy for HTML-only mail: `lynx -dump` / `w3m -T text/html` / built-in strip?
2. Maildir as primary store vs SQLite-only?
3. Ship as single binary name `mailbox` or `mbox` (avoid clash with system `mail`)?
4. Should Ailiak deep-link open `https://ailiak.com/desk` or a future `ailiak://` URL scheme?

---

## 9. References

- Ailiak agent / MCP: `~/Projects/Ailiak.com/agent-api/README.md`
- Ailiak CLI product nouns: `~/Projects/Ailiak.com/docs/cli-product-summary.md`
- Prior art: meli, Himalaya / Pimalaya, aerc (Go), himalaya MCP mode
