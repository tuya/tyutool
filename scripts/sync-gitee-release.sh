#!/usr/bin/env bash
# Sync flattened release assets to a Gitee Release (API v5).
# Env: GITEE_TOKEN, GITEE_USER, GITEE_REPO, TAG, GITEE_ASSETS_DIR
# Optional: GITEE_TARGET_COMMITISH (default: same as TAG; use branch name if tag is not on Gitee yet)
set -euo pipefail

if [[ -z "${GITEE_TOKEN:-}" ]]; then
  echo "::notice::GITEE_TOKEN not set — skipping Gitee release sync."
  exit 0
fi

if [[ -z "${GITEE_REPO:-}" ]]; then
  echo "::error::GITEE_REPO must be set when GITEE_TOKEN is set."
  exit 1
fi
if [[ "${GITEE_REPO}" != */* && -z "${GITEE_USER:-}" ]]; then
  echo "::error::GITEE_USER is required when GITEE_REPO is only the repo name (no '/')."
  exit 1
fi

if [[ ! -d "${GITEE_ASSETS_DIR:-}" ]]; then
  echo "::error::GITEE_ASSETS_DIR is not a directory: ${GITEE_ASSETS_DIR:-}"
  exit 1
fi

OWNER="${GITEE_USER:-}"
REPO="${GITEE_REPO}"
if [[ "${GITEE_REPO}" == */* ]]; then
  OWNER="${GITEE_REPO%%/*}"
  REPO="${GITEE_REPO#*/}"
fi

enc_path() {
  python3 -c "import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1], safe=''))" "$1"
}

tmp_get="$(mktemp)"
tmp_create="$(mktemp)"
tmp_upload_hdr="$(mktemp)"
tmp_attach_list="$(mktemp)"
tmp_delete="$(mktemp)"
tmp_curl_config="$(umask 077; mktemp)"
trap 'rm -f "$tmp_get" "$tmp_create" "$tmp_upload_hdr" "$tmp_attach_list" "$tmp_delete" "$tmp_curl_config"' EXIT

printf 'header = "Authorization: token %s"\n' "$GITEE_TOKEN" > "$tmp_curl_config"

API_BASE="https://gitee.com/api/v5"
ENC_OWNER="$(enc_path "$OWNER")"
ENC_REPO="$(enc_path "$REPO")"
ENC_TAG="$(enc_path "$TAG")"

gitee_curl() {
  curl -sS -K "$tmp_curl_config" "$@"
}

REL_URL="${API_BASE}/repos/${ENC_OWNER}/${ENC_REPO}/releases/tags/${ENC_TAG}"
HTTP_CODE="$(gitee_curl -o "$tmp_get" -w "%{http_code}" "$REL_URL")"

if [[ "$HTTP_CODE" == "200" ]]; then
  RELEASE_ID="$(jq -r '.id // empty' "$tmp_get")"
  if [[ -z "$RELEASE_ID" || "$RELEASE_ID" == "null" ]]; then
    echo "Gitee release lookup returned no id for tag ${TAG}; creating release."
    HTTP_CODE="404"
  else
    echo "Reusing existing Gitee release id=${RELEASE_ID} for tag ${TAG}"
  fi
fi

if [[ "$HTTP_CODE" == "404" ]]; then
  TGT="${GITEE_TARGET_COMMITISH:-$TAG}"
  CREATE_URL="${API_BASE}/repos/${ENC_OWNER}/${ENC_REPO}/releases"
  CREATE_CODE="$(
    jq -n \
      --arg tag "$TAG" \
      --arg name "tyutool ${TAG}" \
      --arg body "Automated sync from GitHub Actions (${GITHUB_REPOSITORY:-unknown})." \
      --arg tc "$TGT" \
      '{tag_name: $tag, name: $name, body: $body, target_commitish: $tc, prerelease: false}' \
      | gitee_curl -o "$tmp_create" -w "%{http_code}" \
      -X POST "$CREATE_URL" \
      -H "Content-Type: application/json" \
      -d @-
  )"

  if [[ "$CREATE_CODE" != "201" && "$CREATE_CODE" != "200" ]]; then
    echo "::error::Create Gitee release failed: HTTP ${CREATE_CODE}"
    jq . "$tmp_create" 2>/dev/null || cat "$tmp_create"
    exit 1
  fi

  RELEASE_ID="$(jq -r '.id // empty' "$tmp_create")"
  if [[ -z "$RELEASE_ID" || "$RELEASE_ID" == "null" ]]; then
    echo "::error::Create response had no release id"
    cat "$tmp_create"
    exit 1
  fi

  echo "Created Gitee release id=${RELEASE_ID}"
elif [[ "$HTTP_CODE" != "200" ]]; then
  echo "::error::GET release by tag failed: HTTP ${HTTP_CODE}"
  jq . "$tmp_get" 2>/dev/null || cat "$tmp_get"
  exit 1
fi

ATTACH_URL="${API_BASE}/repos/${ENC_OWNER}/${ENC_REPO}/releases/${RELEASE_ID}/attach_files"
LIST_CODE="$(gitee_curl -o "$tmp_attach_list" -w "%{http_code}" "$ATTACH_URL")"
if [[ "$LIST_CODE" != "200" ]]; then
  echo "::error::List Gitee release attachments failed: HTTP ${LIST_CODE}"
  jq . "$tmp_attach_list" 2>/dev/null || cat "$tmp_attach_list"
  exit 1
fi

while IFS= read -r attach_id; do
  [[ -n "$attach_id" && "$attach_id" != "null" ]] || continue
  echo "Deleting existing attachment id=${attach_id}"
  DELETE_CODE="$(gitee_curl -o "$tmp_delete" -w "%{http_code}" \
    -X DELETE "${ATTACH_URL}/${attach_id}")"
  if [[ "$DELETE_CODE" != "200" && "$DELETE_CODE" != "204" ]]; then
    echo "::error::Delete Gitee attachment failed for id=${attach_id}: HTTP ${DELETE_CODE}"
    jq . "$tmp_delete" 2>/dev/null || cat "$tmp_delete"
    exit 1
  fi
done < <(jq -r '.[]?.id // empty' "$tmp_attach_list")

shopt -s nullglob
for fpath in "${GITEE_ASSETS_DIR}"/*; do
  [[ -f "$fpath" ]] || continue
  fname="$(basename "$fpath")"
  echo "Uploading: ${fname}"
  UP_CODE="$(gitee_curl -o "$tmp_upload_hdr" -w "%{http_code}" \
    -X POST "$ATTACH_URL" \
    -F "file=@${fpath}")"
  if [[ "$UP_CODE" != "201" && "$UP_CODE" != "200" ]]; then
    echo "::error::Upload failed for ${fname}: HTTP ${UP_CODE}"
    jq . "$tmp_upload_hdr" 2>/dev/null || cat "$tmp_upload_hdr"
    exit 1
  fi
done

echo "Gitee sync finished for ${OWNER}/${REPO} @ ${TAG}"
