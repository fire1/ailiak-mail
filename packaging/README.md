# Packaging AiliakMail (Linux · macOS · Windows)

Installers are built with [cargo-packager](https://github.com/crabnebula-dev/cargo-packager).  
**Each OS package must be built on that OS** (or via CI runners).

## GitLab CI (ready for migration)

[`.gitlab-ci.yml`](../.gitlab-ci.yml) mirrors the GitHub Release workflow:

1. Push a tag `v*` (or run the pipeline manually).
2. Jobs build **Linux** (`.deb` / AppImage), **macOS** (arm64 + Intel `.dmg` / `.app.tar.gz`), **Windows** (NSIS).
3. `publish:release` uploads files to the **Generic Package Registry** and creates a **GitLab Release** with download links.

Default runner tags target **GitLab.com SaaS** (`saas-linux-medium-amd64`, `saas-macos-medium-m1`, `saas-windows-medium-amd64`). On self-hosted GitLab, edit those `tags:` to match your Mac/Windows/Linux runners.

Private projects: friends still need Guest+ access (or a deploy/job token) to download — same as private GitHub.

| Platform | Formats | Icon |
|----------|---------|------|
| **Windows** | NSIS `.exe` installer, optional WiX `.msi` | Embedded in `.exe` via `build.rs` + packager icons |
| **macOS** | `.app` bundle inside a `.dmg` | Packager icons → app bundle |
| **Linux** | `.deb`, AppImage, plus `scripts/install-linux-icon.sh` | FreeDesktop `.desktop` + hicolor PNGs |

## One-time setup

```bash
cargo install cargo-packager --locked
```

### Windows extras

- NSIS: install [NSIS](https://nsis.sourceforge.io/) and ensure `makensis` is on `PATH`
- MSI (optional): [WiX Toolset](https://wixtoolset.org/) v3

### macOS extras

- Xcode command-line tools (`xcode-select --install`)
- Optional code signing: set `macos.signing-identity` in `Packager.toml`

### Linux extras

```bash
# Debian/Ubuntu packaging deps (deb / AppImage)
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxkbcommon-dev libssl-dev pkg-config
```

## Build installers

From the repo root:

```bash
# Windows (on Windows)
cargo packager --release --formats nsis
# cargo packager --release --formats nsis,wix

# macOS (on macOS)
cargo packager --release --formats app,dmg

# Linux
cargo packager --release --formats deb,appimage
# or lightweight local install:
./scripts/install-linux-icon.sh
```

Artifacts land in `dist/`.

## CI

`.github/workflows/release.yml` builds NSIS, DMG, and Deb/AppImage on tagged releases (`v*`).

## Notes

- Linux **cannot** store a dock icon inside the ELF; use the `.desktop` install script or a `.deb`/AppImage from packager.
- Windows Explorer icon comes from the PE resource (`build.rs` + `mailbox.ico`).
- Unsigned macOS/Windows builds may show Gatekeeper / SmartScreen warnings until you add signing certs.
