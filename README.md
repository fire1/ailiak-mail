# Mailbox

**Calm email client for desktop** — Rust + egui. Mail stays local; optional [Ailiak](https://ailiak.com) Desk bridge turns messages into tasks.

[![Release](https://github.com/fire1/mailbox/actions/workflows/release.yml/badge.svg)](https://github.com/fire1/mailbox/actions/workflows/release.yml)

## Features

- **IMAP sync** (Gmail App Password ready) with local SQLite cache  
- **Folders** — Inbox → Sent → Archive → Trash, Lucide-style icons, collapsible rail  
- **Todo** — convert a message into an Ailiak Desk task (slash commands supported)  
- **Calendar** — Desk day view from Ailiak  
- **Themes** — system / light / dark, pastel mint UI  
- **Installers** — Linux, macOS, Windows via `cargo-packager` + GitHub Actions  

## Quick start

```bash
git clone https://github.com/fire1/mailbox.git
cd mailbox

# Debian/Ubuntu deps for egui
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libssl-dev pkg-config

cargo run -p mailbox-gui
```

Or build a release binary for your OS:

```bash
./bin/build-linux.sh      # on Linux  → dist/mailbox-linux-x86_64
./bin/build-macos.sh      # on macOS  → dist/mailbox-macos-*
./bin/build-windows.sh    # on Windows → dist/mailbox-windows-x86_64.exe
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
2. Mailbox → **Settings** → **Use Gmail defaults** → your address  
3. Paste app password → **Save** → **Test IMAP** → **Sync** (`g`)

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
| `mailbox-core` | Config, SQLite cache, IMAP sync, secrets |
| `mailbox-ailiak` | Ailiak MCP client (`create_task`, Desk calendar, slash parse) |
| `mailbox-gui` | egui app (`mailbox` binary) |

## Docs

| Doc | Purpose |
|-----|---------|
| [`docs/VISION.md`](docs/VISION.md) | Product direction |
| [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md) | Requirements & phases |
| [`docs/AILIAK_INTEGRATION.md`](docs/AILIAK_INTEGRATION.md) | Ailiak bridge |

## Paths

- Config: `$XDG_CONFIG_HOME/mailbox/config.toml` (see `config/config.example.toml`)  
- Data: `$XDG_DATA_HOME/mailbox/mail.db`

## License

MIT — see [LICENSE](LICENSE).
