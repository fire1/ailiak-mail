# Integrating AiliakMail ↔ Ailiak

AiliakMail is a **standalone** Rust mail client. Ailiak is the **desk for tasks**.  
Integration should be a thin bridge: *mail → task*, not “Ailiak becomes your inbox.”

---

## 1. Product boundary

| Lives in Mailbox | Lives in Ailiak |
|------------------|-----------------|
| IMAP/SMTP, folders, sync, compose | Desk, Done, Diary, estimates, groups |
| Full message bodies & attachments | Short task line + optional detail |
| Mail account credentials | Agent token (`ailiak_…`) |

**Do not** sync the entire mailbox into Firestore.  
**Do** let the user pick messages (or rules later) to become Desk notes.

Mental model (Ailiak nouns): each sent item is a **note/task** with optional `title`, `text`, `detail`, `forDate`, `embedUrl`, `taskNumber`, hashtags.

---

## 2. Recommended integration (v1)

### Mechanism

Use the existing **Ailiak Agent API / MCP** — same path as Cursor agents and a future CLI.

1. User creates a personal token: **Ailiak → Studio → Integrations → Agents**
2. Mailbox stores:
   - `base_url` (default `https://api.ailiak.com`)
   - token in **OS keyring** (never TOML plaintext)
3. User selects a message → **Send to Ailiak**
4. Mailbox calls MCP tool `create_task` (HTTP JSON-RPC to `/mcp`) with Bearer `ailiak_…`

### Field mapping

| Mail | Ailiak `create_task` |
|------|----------------------|
| Subject (trimmed) | `title` (≤80) and/or short `text` (≤255) |
| From + Date + snippet / plain body | `detail` |
| Stable message URL *or* `mid:` / app deep link | `embedUrl` |
| Optional IMAP UID / Message-ID hash | `taskNumber` (short, ≤24) |
| Today or user-chosen day | `forDate` (`YYYY-MM-DD` or omit for today) |
| Optional `#mail` / `#inbox` | via text/hashtags if API supports (prefer `#mail` in `text`) |

Suggested `text` line:

```text
Reply: <subject>  #mail
```

Suggested `detail`:

```text
From: Ada <ada@example.com>
Date: 2026-11-21
Message-ID: <…@example.com>

<plain body truncated to N KB>
```

### Dedup

Before create:

1. If Mailbox already recorded `message_id → ailiak_task_id` in local SQLite, skip or offer “open existing.”
2. Prefer setting `embedUrl` to a **stable** reference so Ailiak import-style dedup can help later:
   - Best: `https://…` if provider gives a web permalink
   - Else: `mailbox:message-id:<url-encoded-Message-ID>` (documented private scheme until web supports it)

### Auth check

On configure / before first send: call MCP `whoami` (and optionally `get_skill`). Show handle/email in status so the user knows which Desk will receive tasks.

---

## 3. API sketch (Mailbox side)

Feature-gated crate `mailbox-ailiak`:

```text
MailboxAiliakClient
  - whoami() -> Account
  - create_task_from_message(msg, opts) -> TaskId
  - list_open_tasks(for_date?) -> …   // optional, for “already on desk?” UX
```

Transport options (pick one for v1):

| Option | Pros | Cons |
|--------|------|------|
| **A. MCP JSON-RPC over HTTPS** `/mcp` | Matches Cursor; one tool surface | MCP session semantics |
| **B. Thin REST** if/when Ailiak adds public REST for tokens | Simpler from Rust | May not exist yet — MCP is available now |

**v1 decision:** implement **A (MCP)**. Follow `~/Projects/Ailiak.com/agent-api/README.md`.

Config (`$XDG_CONFIG_HOME/mailbox/config.toml`):

```toml
[ailiak]
enabled = true
base_url = "https://api.ailiak.com"
# token lives in keyring service "mailbox" key "ailiak_token"
default_for_date = "today"   # or "null" for unscheduled
hashtag = "mail"
```

---

## 4. UX in Mailbox

| Action | Behavior |
|--------|----------|
| `a` (or command palette) on a message | Send to Ailiak (confirm if body huge) |
| Visual multi-select + `a` | Batch create (rate-limit politely) |
| Status | `Ailiak: created #123…` or error |
| Settings | Paste token once → keyring; Test (`whoami`) |

Out of scope for v1:

- Two-way sync (marking Desk done does not archive IMAP)
- Auto-import all mail
- Rendering Ailiak Desk inside Mailbox

---

## 5. Optional later bridges

### 5.1 Ailiak → Mailbox (deep link)

- Task `embedUrl` opens Mailbox via `mailbox://open?message-id=…` (register `.desktop` mime/url handler on Linux).
- Or open provider webmail URL if `embedUrl` is https.

### 5.2 Himalaya-style scripting

- CLI: `mailbox ailiak send --id <uid>` for scripts/agents.
- Compose with Mailbox, track in Ailiak.

### 5.3 Studio “Sync” style (inside Ailiak web)

Only if product wants browser-side mail later — **separate** from this Rust app. Prefer keeping mail offline in Mailbox.

### 5.4 Shared export file

Fallback without API: export selected messages as CSV matching Ailiak **File → CSV Import**. Worse UX; keep as emergency escape hatch.

---

## 6. Security

1. Agent token = full desk access for that user (or group scope). Treat like a password.
2. Store only in keyring; allow `mailbox ailiak logout` to delete.
3. Never upload attachments to Ailiak unless user opts in (v1: text only).
4. Truncate `detail` (e.g. 32–64 KiB) to avoid huge posts.
5. TLS to `api.ailiak.com` only; no custom CA mute without explicit config.

---

## 7. Implementation tasks (integration track)

- [x] **I1** Config + keyring for `ailiak_token` + `base_url`
- [x] **I2** MCP client: `initialize`, `whoami`, `create_task`
- [x] **I3** Map `Message` → create_task payload + local dedup table
- [x] **I4** GUI action + status feedback (TUI binding later)
- [x] **I5** Docs: user setup (Agents token → Mailbox)
- [ ] **I6** (Later) `mailbox://` URL handler + open-from-Ailiak
- [ ] **I7** (Later) Batch send + dry-run

**Integration done when:** from a synced IMAP message, one keystroke creates a Desk task visible on ailiak.com with subject and enough detail to act, without copying mail passwords to Ailiak.

---

## 8. What Ailiak may add later (nice, not blocking)

| Enhancement | Why |
|-------------|-----|
| Official REST `/v1/tasks` for PATs | Simpler than MCP for native apps |
| Documented `embedUrl` schemes for local apps | Cleaner deep links |
| Studio Integrations → **Mailbox** help card | Discoverability |
| Idempotent create by `embedUrl` server-side | Stronger dedup |

Mailbox v1 should work with **today’s** MCP `create_task` + `whoami` only.
