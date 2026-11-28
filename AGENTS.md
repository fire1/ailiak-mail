# Agent notes — AiliakMail

## Theme: always light + dark

When changing **any visual** (colors, fills, strokes, text ink, chips, cards, avatars, overlays, webview chrome, row backgrounds):

1. **Update both variants** — light and dark — in the same change. Do not ship a fix that only looks right in one theme.
2. Prefer tokens in `crates/mailbox-gui/src/theme.rs` (`Palette`, `light_palette` / `dark_palette`, helpers like `ink`, `mute`, `unread_row`). Avoid hard-coded `Color32::WHITE` / fixed greys for shell UI that must track theme.
3. **Exceptions that stay light on purpose** (keep a comment if you touch them):
   - HTML mail reader card / webview backdrop (`reader_card`, `mail_webview_backdrop`) — message HTML is authored for light paper.
4. After a visual change, sanity-check: unread list rows, selection, mute text, and avatars in **both** themes.
5. Unread list rows use `unread_row` (theme-aware). Do not force a white unread plate in dark mode.

## No borders / strokes on controls

Do **not** put borders or outline strokes on interactive elements (text fields, textareas, buttons, chips, inputs, frames around edits). Prefer fill, spacing, typography, and focus background only.

- Options NOTE / TASK fields: fill + focus plate, **no** `Frame`/`TextEdit` stroke.
- Avoid `Stroke` boxes around widgets unless the existing design system already requires a structural divider (e.g. panel card edge, list separators) — not control chrome.
