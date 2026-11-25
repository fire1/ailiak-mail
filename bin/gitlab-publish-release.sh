#!/usr/bin/env bash
# Upload dist/* to GitLab Generic Package Registry and create a Release.
# Intended for CI (needs CI_JOB_TOKEN, CI_PROJECT_ID, CI_COMMIT_TAG, release-cli).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

: "${CI_JOB_TOKEN:?}"
: "${CI_API_V4_URL:?}"
: "${CI_PROJECT_ID:?}"
: "${CI_COMMIT_TAG:?}"

PACKAGE_NAME="${PACKAGE_NAME:-ailiakmail}"
DIST_DIR="${DIST_DIR:-dist}"

mkdir -p "$DIST_DIR"
ls -la "$DIST_DIR" || true

count=0
link_args=()

while IFS= read -r -d '' f; do
  name="$(basename "$f")"
  echo "Uploading $name …"
  curl --fail --silent --show-error \
    --header "JOB-TOKEN: ${CI_JOB_TOKEN}" \
    --upload-file "$f" \
    "${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/packages/generic/${PACKAGE_NAME}/${CI_COMMIT_TAG}/${name}"
  url="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/packages/generic/${PACKAGE_NAME}/${CI_COMMIT_TAG}/${name}"
  link_args+=(--assets-link "$(printf '{"name":"%s","url":"%s","link_type":"package"}' "$name" "$url")")
  count=$((count + 1))
done < <(find "$DIST_DIR" -type f -print0)

if [[ "$count" -eq 0 ]]; then
  echo "error: no files in ${DIST_DIR}/ to publish" >&2
  exit 1
fi

release-cli create \
  --name "AiliakMail ${CI_COMMIT_TAG}" \
  --description "Desktop installers (Linux / macOS arm64+Intel / Windows). Private projects require project access to download." \
  --tag-name "${CI_COMMIT_TAG}" \
  "${link_args[@]}"

echo "✓ GitLab Release ${CI_COMMIT_TAG} (${count} assets)"
