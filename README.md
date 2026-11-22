# AiliakMail

**Calm email client for desktop** — Rust + egui. Mail stays local; optional [Ailiak](https://ailiak.com) Desk bridge turns messages into tasks.

[![Release](https://github.com/fire1/ailiakmail/actions/workflows/release.yml/badge.svg)](https://github.com/fire1/ailiakmail/actions/workflows/release.yml)

## Features

- **IMAP sync** (Gmail App Password ready) with local SQLite cache  
- **POP3** for older mail servers (POP3S :995 or plain :110; mail left on server)  
- **Folders** — Inbox → Sent → Archive → Trash, Lucide-style icons, collapsible rail  
- **Multi-account** — avatar strip + folders in one nav column  
- **Todo** — convert a message into an Ailiak Desk task (slash commands supported)  
- **Calendar** — Desk day view from Ailiak  
- **Themes** — system / light / dark, pastel mint UI  
- **Installers** — Linux, macOS, Windows via `cargo-packager` + GitHub Actions  

## Quick start

```bash
git clone https://github.com/fire1/ailiakmail.git
cd ailiakmail

# Debian/Ubuntu deps for egui
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libssl-dev pkg-config

cargo run -p mailbox-gui
```

Or build a release binary for your OS:

```bash
./bin/build-linux.sh      # on Linux  → dist/ailiakmail-linux-x86_64
./bin/build-macos.sh      # on macOS  → dist/ailiakmail-macos-*
./bin/build-windows.sh    # on Windows → dist/ailiakmail-windows-x86_64.exe
./bin/build-all.sh        # whatever this host can build
```

Add `--package` to also produce installers (`.deb` / AppImage / `.dmg` / NSIS) when tooling is available.

## Build scripts (`bin/`)

| Script | Platform | Notes |
|--------|----------|--------|
| [`bin/build-linux.sh`](bin/build-linux.sh) | Linux | Native release; `--package` → deb + AppImage |
| [`bin/build-macos.sh`](bin/build-macos.sh) | macOS | Native; `--universal` lipo; `--package` → app + dmg |
| [`bin/build-windows.sh`](bin/build-windows.sh) | Windows | Native MSVC; `--cross` from Linux (experimental); `--package` → NSIS |
| [`bin/build-all.sh`](bin/build-all.sh) | current host | Runs the matching script above |

Cross-compiling a full egui GUI to another OS is unreliable; for release artifacts use **GitHub Actions** (tag `v*` → `.github/workflows/release.yml`).

## Gmail

1. Google Account → **Security** → 2FA → **App password**  
2. AiliakMail → **Settings** → **Use Gmail defaults** → your address  
3. Paste app password → **Save** → **Test IMAP** → **Sync** (`g`)

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
