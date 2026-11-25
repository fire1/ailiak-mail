# AiliakMail

**Calm email client for desktop** — Rust + egui. Mail stays local; optional [Ailiak](https://ailiak.com) Desk bridge turns messages into tasks.

[![Release](https://github.com/fire1/ailiak-mail/actions/workflows/release.yml/badge.svg)](https://github.com/fire1/ailiak-mail/actions/workflows/release.yml)

## Features

- **IMAP sync** (Sign in with Google / App Password) with local SQLite cache  
- **POP3** for older mail servers (POP3S :995 or plain :110; mail left on server)  
- **Folders** — Inbox → Sent → Archive → Trash, Lucide-style icons, collapsible rail  
- **Multi-account** — avatar strip + folders in one nav column  
- **Todo** — convert a message into an Ailiak Desk task (slash commands supported)  
- **Calendar** — Desk day view from Ailiak  
- **Themes** — system / light / dark, pastel mint UI  
- **Installers** — Linux, macOS, Windows via `cargo-packager` + GitHub Actions  

## Download the latest version

Pre-built installs are on GitHub Releases (newest tag first):

**https://github.com/fire1/ailiak-mail/releases/latest**

| OS | What to download |
|----|------------------|
| **macOS** (Apple Silicon) | `AiliakMail_*_aarch64.dmg` or `AiliakMail-aarch64.app.tar.gz` |
| **macOS** (Intel) | `AiliakMail_*_x64.dmg` / `*_x86_64.dmg` or `AiliakMail-x86_64.app.tar.gz` |
| **Windows** | `ailiakmail_*_x64-setup.exe` (or `ailiakmail.exe`) |
| **Linux** | `.deb` / AppImage when present on that release |

Open the release page → **Assets** → pick the file for your OS. macOS CI builds both **arm64** and **Intel (x86_64)**.

To build from source instead, see [Quick start](#quick-start) below.

## Quick start

```bash
git clone https://github.com/fire1/ailiak-mail.git
cd ailiak-mail

# Debian/Ubuntu deps for egui + HTML mail webview (WebKitGTK)
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libssl-dev pkg-config \
  libgtk-3-dev libwebkit2gtk-4.1-dev

cargo run -p mailbox-gui
```

HTML messages render in an embedded OS webview (**wry**: WebKitGTK on Linux, WKWebView on macOS, WebView2 on Windows). Remote images stay blocked until you allow them for a sender (toolbar image icon). After upgrading, **Sync all** so messages get `body_html` stored. On Linux, the app forces **X11/XWayland** for both winit and GDK (`GDK_BACKEND=x11`) because WebKitGTK child webviews do not support native Wayland handles; if the webview still fails to create, the plain-text reader is used as fallback.


Or build a release binary for your OS:

```bash
./bin/build-linux.sh      # on Linux  → dist/ailiakmail-linux-x86_64
./bin/build-macos.sh      # on macOS  → dist/ailiakmail-macos-*
./bin/build-windows.sh    # on Windows → dist/ailiakmail-windows-x86_64.exe
./bin/build-all.sh        # whatever this host can build
```

Add `--package` to also produce installers (`.deb` / AppImage / `.dmg` / NSIS) when tooling is available.

### Publish a multi-OS GitHub Release

One machine cannot reliably build Linux + macOS + Windows GUIs. Use the release script to **tag and push**; GitHub Actions then builds all platforms and attaches installers:

```bash
# commit your work first, then:
./bin/release.sh 0.1.2           # creates v0.1.2, pushes, triggers CI
./bin/release.sh 0.1.2 --watch   # same, then waits for the Actions run
./bin/release.sh --from-cargo    # use version from Cargo.toml [workspace.package]
./bin/release.sh 0.1.2 --dry-run # preview only
```

## Build scripts (`bin/`)

| Script | Platform | Notes |
|--------|----------|--------|
| [`bin/build-linux.sh`](bin/build-linux.sh) | Linux | Native release; `--package` → deb + AppImage |
| [`bin/build-macos.sh`](bin/build-macos.sh) | macOS | Native; `--universal` lipo; `--package` → app + dmg |
| [`bin/build-windows.sh`](bin/build-windows.sh) | Windows | Native MSVC; `--cross` from Linux (experimental); `--package` → NSIS |
| [`bin/build-all.sh`](bin/build-all.sh) | current host | Runs the matching script above |
| [`bin/release.sh`](bin/release.sh) | all (via CI) | Tag `v*`, push, GitHub Actions → Linux + macOS arm64/Intel + Windows |

Cross-compiling a full egui GUI to another OS is unreliable; for release artifacts use **`./bin/release.sh`** (or push a `v*` tag → `.github/workflows/release.yml`).

**GitLab:** the same multi-OS packaging is in [`.gitlab-ci.yml`](.gitlab-ci.yml) (tag `v*` → Linux + macOS arm64/Intel + Windows → Package Registry + Release). See [`packaging/README.md`](packaging/README.md). On migrate day, push tags to GitLab; adjust runner `tags:` if you are not on GitLab.com SaaS.

## App authorization (recommended)

Supported: **Gmail**, **Outlook / Microsoft 365**, **Yahoo**, **Yandex**.  
(iCloud and GMX use app-specific / account passwords only.)

1. **Add account** → pick provider → **Authorize with …**  
2. Complete login / 2FA in the browser (you are authorizing **AiliakMail**, not pasting a mail password)  
3. **Sync** (`g`)

Release builds should embed AiliakMail’s OAuth Client IDs (users never paste them). Maintainers: see [`docs/oauth-apps.md`](docs/oauth-apps.md).

## POP3 (older servers)

1. Settings → **Use POP3 defaults** (or set Protocol → POP3)  
2. Enter host, port (`995` + TLS, or `110` plain), username, password  
3. **Save** → **Test POP3** → **Sync** (`g`)  
4. Inbox only — mail stays on the server (no DELE)

See [`config/config.example.toml`](config/config.example.toml) for a TOML sketch.

## Ailiak

1. Ailiak → Studio → AI agents → create `ailiak_…` token  
2. Settings → paste token → Save → Test  
3. Open a message → **Todo** (`a`) or open **Calendar** (`c`)

## Installers & icons

```bash
cargo install cargo-packager --locked
cargo packager --release --formats nsis          # Windows
cargo packager --release --formats app,dmg       # macOS
cargo packager --release --formats deb,appimage  # Linux

# Linux dock icon without a full package:
./scripts/install-linux-icon.sh
```

Details: [`packaging/README.md`](packaging/README.md)

## Crates

| Crate | Role |
|-------|------|
| `mailbox-core` | Config, SQLite cache, IMAP/POP3 sync, secrets |
| `mailbox-ailiak` | Ailiak MCP client (`create_task`, Desk calendar, slash parse) |
| `mailbox-gui` | egui app (`ailiakmail` binary) |
| `mailbox-mcp` | stdio MCP server (`ailiakmail-mcp` binary) |

## MCP (agents)

Expose the local AiliakMail SQLite cache to Cursor / other MCP clients:

```bash
cargo build -p mailbox-mcp --release
```

Cursor `mcp.json` (stdio):

```json
{
  "mcpServers": {
    "ailiakmail": {
      "command": "/absolute/path/to/target/release/ailiakmail-mcp"
    }
  }
}
```

Tools: `list_accounts`, `list_folders`, `list_messages`, `search_messages`, `get_message`, `mark_seen`, `sync_folder`.

## Docs

| Doc | Purpose |
|-----|---------|
| [`docs/VISION.md`](docs/VISION.md) | Product direction |
| [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md) | Requirements & phases |
| [`docs/AILIAK_INTEGRATION.md`](docs/AILIAK_INTEGRATION.md) | Ailiak bridge |

## Paths

- Config: `$XDG_CONFIG_HOME/ailiakmail/config.toml` (legacy `mailbox/` still used if present)  
- Data: `$XDG_DATA_HOME/ailiakmail/mail.db`  
- See also [`config/config.example.toml`](config/config.example.toml)

## License

MIT — see [LICENSE](LICENSE).
