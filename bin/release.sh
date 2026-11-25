#!/usr/bin/env bash
# Cut a GitHub Release that builds Linux + macOS (arm64 & Intel) + Windows via CI.
#
# Full multi-OS installers cannot be produced on one machine (egui/winit). Pushing a
# `v*` tag runs `.github/workflows/release.yml`, which packages all platforms and
# attaches artifacts to the GitHub Release.
#
# Usage:
#   ./bin/release.sh 0.1.2              # tag v0.1.2, push, trigger CI
#   ./bin/release.sh v0.1.2 --watch     # same, then wait for the Actions run
#   ./bin/release.sh --from-cargo       # use [workspace.package] version from Cargo.toml
#   ./bin/release.sh 0.1.2 --local      # also run ./bin/build-all.sh --package on this host
#   ./bin/release.sh 0.1.2 --dry-run    # print steps only
#   ./bin/release.sh 0.1.2 --no-push    # create annotated tag locally, do not push
#
# Prerequisites: git, gh (logged in), clean tree (or --allow-dirty).
set -euo pipefail
# shellcheck source=./_common.sh
source "$(cd "$(dirname "$0")" && pwd)/_common.sh"

VERSION=""
FROM_CARGO=0
WATCH=0
LOCAL=0
DRY_RUN=0
NO_PUSH=0
ALLOW_DIRTY=0

usage() {
  cat <<'EOF'
Cut a GitHub Release: tag + push → CI builds Linux, macOS (arm64 + Intel), Windows.

Usage:
  ./bin/release.sh 0.1.2              # tag v0.1.2, push, trigger CI
  ./bin/release.sh v0.1.2 --watch     # same, then wait for the Actions run
  ./bin/release.sh --from-cargo       # use version from Cargo.toml
  ./bin/release.sh 0.1.2 --local      # also ./bin/build-all.sh --package (this host)
  ./bin/release.sh 0.1.2 --dry-run    # print steps only
  ./bin/release.sh 0.1.2 --no-push    # local tag only

Options:
  --from-cargo      Read [workspace.package] version
  --watch, -w       gh run watch until Release finishes
  --local, -l       Build packages for this OS before tagging
  --dry-run, -n     Do not tag/push
  --no-push         Create annotated tag locally only
  --allow-dirty     Allow uncommitted changes
  --version VER     Same as passing VER as the first argument
  -h, --help        Show this help

Needs: git, gh (logged in), clean working tree (unless --allow-dirty).
EOF
  exit 0
}

cargo_workspace_version() {
  # First `version = "…"` under [workspace.package] in root Cargo.toml.
  awk '
    /^\[workspace\.package\]/ { in_pkg=1; next }
    /^\[/ { in_pkg=0 }
    in_pkg && /^version[[:space:]]*=/ {
      if (match($0, /"[^"]+"/)) {
        print substr($0, RSTART+1, RLENGTH-2)
        exit
      }
    }
  ' "$ROOT/Cargo.toml"
}

normalize_tag() {
  local v="$1"
  v="${v#v}"
  if [[ ! "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.]+)?$ ]]; then
    echo "error: version must look like 0.1.2 or v0.1.2 (got: $1)" >&2
    exit 1
  fi
  echo "v${v}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage ;;
    --from-cargo) FROM_CARGO=1 ;;
    --watch|-w) WATCH=1 ;;
    --local|-l) LOCAL=1 ;;
    --dry-run|-n) DRY_RUN=1 ;;
    --no-push) NO_PUSH=1 ;;
    --allow-dirty) ALLOW_DIRTY=1 ;;
    --version)
      shift
      VERSION="${1:-}"
      ;;
    -*)
      echo "error: unknown option: $1 (try --help)" >&2
      exit 1
      ;;
    *)
      if [[ -n "$VERSION" ]]; then
        echo "error: unexpected argument: $1" >&2
        exit 1
      fi
      VERSION="$1"
      ;;
  esac
  shift
done

if [[ "$FROM_CARGO" -eq 1 ]]; then
  VERSION="$(cargo_workspace_version)"
  if [[ -z "$VERSION" ]]; then
    echo "error: could not read workspace version from Cargo.toml" >&2
    exit 1
  fi
fi

if [[ -z "$VERSION" ]]; then
  echo "error: pass a version (e.g. 0.1.2) or --from-cargo" >&2
  echo "       try: $0 --help" >&2
  exit 1
fi

TAG="$(normalize_tag "$VERSION")"
need_cmd git

if [[ "$ALLOW_DIRTY" -eq 0 ]]; then
  if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree is dirty — commit/stash first, or pass --allow-dirty" >&2
    git status -sb
    exit 1
  fi
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
REMOTE="${REMOTE:-origin}"
HEAD_SHA="$(git rev-parse HEAD)"

if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "error: tag already exists locally: $TAG" >&2
  exit 1
fi

if git ls-remote --tags "$REMOTE" "refs/tags/$TAG" 2>/dev/null | grep -q .; then
  echo "error: tag already exists on $REMOTE: $TAG" >&2
  exit 1
fi

echo "AiliakMail release"
echo "  branch:  $BRANCH @ ${HEAD_SHA:0:8}"
echo "  tag:     $TAG"
echo "  remote:  $REMOTE"
echo "  CI:      push tag → Release workflow (Linux + macOS arm64/Intel + Windows)"
echo

run() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "dry-run: $*"
  else
    "$@"
  fi
}

if [[ "$LOCAL" -eq 1 ]]; then
  echo "→ host packages (this OS only)"
  run ./bin/build-all.sh --package
  echo
fi

MSG="Release $TAG"
run git tag -a "$TAG" -m "$MSG"
if [[ "$DRY_RUN" -eq 0 ]]; then
  echo "✓ local tag $TAG"
else
  echo "(dry-run) would create local tag $TAG"
fi

if [[ "$NO_PUSH" -eq 1 ]]; then
  echo
  echo "Tag created locally only (--no-push)."
  echo "When ready:  git push $REMOTE HEAD && git push $REMOTE $TAG"
  exit 0
fi

need_cmd gh
if [[ "$DRY_RUN" -eq 0 ]] && ! gh auth status >/dev/null 2>&1; then
  echo "error: gh is not logged in — run: gh auth login" >&2
  exit 1
fi

# Push branch tip first so the tagged commit is on the remote.
if [[ "$(git rev-parse "$BRANCH")" == "$HEAD_SHA" ]]; then
  run git push "$REMOTE" "HEAD:refs/heads/$BRANCH"
fi
run git push "$REMOTE" "$TAG"

echo
if [[ "$DRY_RUN" -eq 0 ]]; then
  echo "✓ pushed $TAG — GitHub Actions will attach installers to:"
else
  echo "(dry-run) would push $TAG — CI would attach installers to:"
fi
echo "  https://github.com/fire1/ailiak-mail/releases/tag/$TAG"
echo "  https://github.com/fire1/ailiak-mail/actions/workflows/release.yml"

if [[ "$WATCH" -eq 1 ]]; then
  echo
  echo "→ waiting for Release workflow…"
  # Give Actions a moment to register the run for this tag.
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "dry-run: gh run watch (Release / $TAG)"
  else
    sleep 3
    RUN_ID="$(
      gh run list --workflow=release.yml --branch "$TAG" --limit 1 --json databaseId --jq '.[0].databaseId // empty'
    )"
    if [[ -z "$RUN_ID" ]]; then
      # Fallback: newest Release run
      RUN_ID="$(
        gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId // empty'
      )"
    fi
    if [[ -n "$RUN_ID" ]]; then
      gh run watch "$RUN_ID" --exit-status
      echo
      gh release view "$TAG" || true
    else
      echo "warning: could not find a Release run yet — check Actions manually" >&2
    fi
  fi
fi

echo
echo "Done."
