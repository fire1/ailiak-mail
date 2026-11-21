# Mailbox — product vision

**One-liner:** Read mail as calm conversation threads; turn anything into an Ailiak Desk task in one motion — with day, slash commands, and control, without making Ailiak a second inbox.

Reference implementations:

| Source | What we borrow |
|--------|----------------|
| [Ailiak.com](https://ailiak.com) (`~/Projects/Ailiak.com`) | Desk / day model, slash compose dialect, accent & tone, Agent MCP |
| Talanoa | Conversation-style grouping of messages (chat timeline, not a flat inbox list) |

Mailbox stays a **standalone Linux mail client**. Ailiak stays the **calm desk**. The bridge is intentional and thin: *selected mail → structured work*.

---

## 1. Problem

Classic MUAs make “this email is work” a chore: copy subject, open another app, paste, pick a day, forget context.  
Mailbox should make that path feel like **one continuous action** — as natural as replying.

---

## 2. Principles

1. **Mail lives in Mailbox** — full bodies, folders, sync, offline cache.
2. **Work lives in Ailiak** — Desk notes/tasks, days, estimates, Done, Diary.
3. **Seamless > powerful** — one primary action (**Todo**) beats a settings maze.
4. **Conversation first** — threads read like chat; tasks hang off the message that matters.
5. **Same language as Ailiak** — Todo compose speaks the Desk slash dialect (`/from`, `/estimate`, …).
6. **Optional bridge** — no token ⇒ still a great mail reader; Ailiak never blocks reading.

---

## 3. Core experience

### 3.1 Mail as chat (Talanoa-like)

Instead of only a Thunderbird-style 3-pane list:

- Group messages into **threads** (Message-ID / In-Reply-To / References).
- Present a thread as a **vertical conversation**: bubbles or stacked turns, clear “them / me”, timestamps, unread markers.
- Opening a thread focuses the latest (or selected) turn; older turns scroll above like a chat history.
- Flat folder list remains available for power users; **thread/chat view is the default read path**.

Goal: scanning a long reply chain feels like reading a chat, then pinning work out of it — not hunting envelopes.

### 3.2 Todo — one seamless action

Primary CTA: **Todo** (keyboard: `a` or dedicated binding).

From a message or multi-select:

1. User hits **Todo** (no mandatory wizard for the happy path).
2. Mailbox opens a **compact Todo strip / sheet** prefilled from the message:
   - Short line / title ← subject (trimmed)
   - Detail ← From, Date, Message-ID, plain body snippet
   - `#mail` (or configured hashtag)
   - Stable link / id for dedup (`embedUrl` or local `mailbox:…` / taskNumber)
3. User can **send immediately**, or tweak **day + slash line** in the same strip.
4. Result lands on Ailiak Desk; status shows success (task id) or “already on Desk”.

**Seamless means:**

- Zero copy-paste.
- Defaults are good enough for one keypress.
- Dedup so the same Message-ID does not spawn clones.
- Token + IMAP stay in the OS keyring; mail passwords never go to Ailiak.

### 3.3 Calendar / day (align with Ailiak Desk)

Ailiak schedules work with `forDate` (`YYYY-MM-DD`). Mailbox must speak that natively:

| UX | Behavior |
|----|----------|
| Mini calendar in Todo strip | Pick the Desk day before create |
| Quick chips | Today · Tomorrow · Unscheduled |
| Slash in Todo line | `/todo`, `/from tomorrow`, `/date 2026-11-25` — same as Ailiak compose |
| Optional Calendar pane | Month/week overview of **mail-derived** tasks already linked (read via MCP `list_open_tasks` / local links) — not a full CalDAV suite in v1 |

Mailbox does **not** replace Google Calendar. It only schedules **Ailiak tasks that came from mail** onto Desk days.

### 3.4 Slash commands in Todo (Ailiak compose parity)

Ailiak’s composer parses one field into structured task fields. Mailbox Todo must accept the **same dialect** so muscle memory transfers.

Source of truth in Ailiak: `src/utils/composeCommands.ts` and Studio → Help.

| Command | Effect (Mailbox → Ailiak create / update) |
|---------|-------------------------------------------|
| `/title …` | Task title (until newline) |
| `/help …` | Single-line detail |
| `/details …` | Multi-line detail (prefer last) |
| `/todo` | Schedule **tomorrow** |
| `/from` · `/date` | Day: `today`, `tomorrow`, `MM-DD`, `yy-mm-dd`, `YYYY-MM-DD` |
| `/draft` · `/tpl` | Create as draft/template (if API allows; else document gap) |
| `/estimate 1h 30m` | `estimateSeconds` |
| `/spent` · `/spend` | `spentSeconds` (on create or follow-up note) |
| `/done` | Create already done, or mark after create |
| `#tag` | Hashtags (e.g. `#mail`) |
| `+slug` / `-slug` | Group attach (when token/API supports it) |

**Todo strip UX:**

- One input line (255-char short text) + expandable detail.
- Autocomplete / cheatsheet for `/` and `#` (mirror Ailiak ComposePost hints).
- Parser shared conceptually with Ailiak; Mailbox maps parse result → MCP `create_task` (+ `set_task_date`, `set_task_estimate`, `add_task_spent`, `mark_task_done` as needed).

After create, optional **follow-up commands** on the linked task (e.g. open “linked task” → type `/spent 15m`) so control stays inside Mailbox without opening the browser for every tweak.

---

## 4. Product boundary

| In Mailbox | In Ailiak |
|------------|-----------|
| IMAP/SMTP, folders, sync, cache | Desk, Done, Diary, Groups, estimates |
| Thread/chat reading UI | Day timeline & organize day |
| Todo strip + slash + mini calendar | Canonical task store |
| Local `message_id → task_id` dedup | Agent token desk scope |
| Optional “open on Desk” deep link | Web/SPA experience |

**Out of scope (v1):** hosting mail on Ailiak, embedding the Ailiak SPA inside Mailbox, auto-importing the entire inbox, full CalDAV/contacts suite.

---

## 5. UX sketch

```
┌──────────────┬─────────────────────┬────────────────────────────┐
│ Folders      │ Thread list         │ Conversation (chat)        │
│              │ (subject + preview) │  ┌─ them ───────────────┐  │
│              │                     │  └──────────────────────┘  │
│              │                     │     ┌─ me ──────────────┐  │
│              │                     │     └───────────────────┘  │
│              │                     │  [Todo]  [Reply]  …         │
├──────────────┴─────────────────────┴────────────────────────────┤
│ Ailiak · today · synced …          Todo strip (when open):       │
│                                    short line + /commands + day  │
└─────────────────────────────────────────────────────────────────┘
```

- **Todo** in chrome (top or beside conversation) — create/control.
- **Ailiak (off/on)** bottom-left — bridge status / open settings (already started).
- Calendar: chip strip + optional side month for day pick.

---

## 6. Technical shape (unchanged rules)

```
mailbox/
  crates/
    mailbox-core/     # IMAP, store, threads, config, keyring
    mailbox-ailiak/   # MCP: whoami, create_task, set_task_date, …
    mailbox-gui/      # egui: chat threads, Todo strip, calendar
```

- UI never talks IMAP or MCP directly — only through core / ailiak crates.
- Secrets in OS keyring; TLS by default.
- Config/data under XDG.

---

## 7. Delivery phases (vision → build)

| Phase | Outcome |
|-------|---------|
| **A — Bridge solid** | Gmail/IMAP read + Todo sheet create + dedup + status — **in app** |
| **B — Chat threads** | Thread grouping + Talanoa-like conversation view as default read |
| **C — Day & calendar** | Todo day chips + Desk calendar pane (`forDate`) + refresh open tasks — **in app** |
| **D — Slash control** | Compose dialect in Todo (`/from`, `/todo`, `/estimate`, `/spent`, `/done`, `#`) — **in app** |
| **E — Polish** | Batch Todo, notifications, packaging, deeper Ailiak deep links |

---

## 8. Success looks like

1. User reads a thread as a chat, hits **Todo**, task appears on today’s (or chosen) Ailiak Desk with enough detail to act — under ~3 seconds of thought.
2. User types `/from tomorrow /estimate 1h #mail` in the Todo strip and gets the same scheduling semantics as Ailiak Desk.
3. Same email never creates two Desk tasks.
4. With Ailiak off, Mailbox still works as a calm offline mail reader.

---

## 9. Open questions

1. Default Todo day: always today, or infer from mail Date / “follow-up” heuristics?
2. How close should chat bubbles match Talanoa visually vs stay “mail-native”?
3. Should `/draft` create Ailiak drafts via API, or only full Desk notes in v1?
4. Batch: multi-select thread turns → one task vs one task per message?

---

## 10. Related docs

- [`REQUIREMENTS.md`](REQUIREMENTS.md) — phased engineering requirements  
- [`AILIAK_INTEGRATION.md`](AILIAK_INTEGRATION.md) — MCP bridge details  
- Ailiak: `docs/cli-product-summary.md`, Studio Help, `src/utils/composeCommands.ts`
