#!/usr/bin/env bash
# Idempotently publish one firmware-asset family to a Gitee Release (API v5).
#
# Shared by both families — auth-firmware (assets/auth-firmware, tag auth-firmware)
# and ram-loader (assets/ram-loader, tag ram-loader). Only the env below differs;
# the release plumbing is identical, so it lives here once.
#
# Preserves immutable firmware:
#   - <name>.bin        : skip if an attachment with the same name already exists, else upload.
#   - <MANIFEST_FILE>   : delete the old attachment (if any), then upload (overwrite).
#
# Env: GITEE_TOKEN, GITEE_USER, GITEE_REPO
# Optional: TAG (default auth-firmware), ASSET_DIR (default assets/<TAG>),
#           MANIFEST_FILE (default <TAG>.json),
#           RELEASE_NAME / RELEASE_BODY (only used when creating the release),
#           GITEE_TARGET_COMMITISH (default: the tag; use a branch name if the tag isn't on Gitee yet)
set -euo pipefail

if [[ -z "${GITEE_TOKEN:-}" ]]; then
  echo "::notice::GITEE_TOKEN not set — skipping Gitee firmware-asset publish."
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

TAG="${TAG:-auth-firmware}"
ASSET_DIR="${ASSET_DIR:-assets/${TAG}}"
MANIFEST_FILE="${MANIFEST_FILE:-${TAG}.json}"
MANIFEST_NAME="$(basename "$MANIFEST_FILE")"
RELEASE_NAME="${RELEASE_NAME:-${TAG}}"
RELEASE_BODY="${RELEASE_BODY:-资产由 release-${TAG} workflow 自动维护。}"

if [[ ! -d "$ASSET_DIR" ]]; then
  echo "::error::ASSET_DIR is not a directory: ${ASSET_DIR}"
  exit 1
fi
if [[ ! -f "$MANIFEST_FILE" ]]; then
  echo "::error::MANIFEST_FILE not found: ${MANIFEST_FILE}"
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
tmp_attach_list="$(mktemp)"
tmp_resp="$(mktemp)"
tmp_curl_config="$(umask 077; mktemp)"
trap 'rm -f "$tmp_get" "$tmp_create" "$tmp_attach_list" "$tmp_resp" "$tmp_curl_config"' EXIT

printf 'header = "Authorization: token %s"\n' "$GITEE_TOKEN" > "$tmp_curl_config"

API_BASE="https://gitee.com/api/v5"
ENC_OWNER="$(enc_path "$OWNER")"
ENC_REPO="$(enc_path "$REPO")"
ENC_TAG="$(enc_path "$TAG")"

gitee_curl() {
  curl -sS -K "$tmp_curl_config" "$@"
}

# --- Resolve or create the release -----------------------------------------
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
      --arg name "$RELEASE_NAME" \
      --arg body "$RELEASE_BODY" \
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

# --- List existing attachments ---------------------------------------------
ATTACH_URL="${API_BASE}/repos/${ENC_OWNER}/${ENC_REPO}/releases/${RELEASE_ID}/attach_files"
LIST_CODE="$(gitee_curl -o "$tmp_attach_list" -w "%{http_code}" "$ATTACH_URL")"
if [[ "$LIST_CODE" != "200" ]]; then
  echo "::error::List Gitee release attachments failed: HTTP ${LIST_CODE}"
  jq . "$tmp_attach_list" 2>/dev/null || cat "$tmp_attach_list"
  exit 1
fi

attachment_id_by_name() {
  jq -r --arg n "$1" '.[]? | select(.name == $n) | .id' "$tmp_attach_list" | head -n1
}
attachment_exists() {
  [[ -n "$(attachment_id_by_name "$1")" ]]
}

upload_attachment() {
  local fpath="$1"
  UP_CODE="$(gitee_curl -o "$tmp_resp" -w "%{http_code}" \
    -X POST "$ATTACH_URL" \
    -F "file=@${fpath}")"
  if [[ "$UP_CODE" != "201" && "$UP_CODE" != "200" ]]; then
    echo "::error::Upload failed for $(basename "$fpath"): HTTP ${UP_CODE}"
    jq . "$tmp_resp" 2>/dev/null || cat "$tmp_resp"
    exit 1
  fi
}

# --- Firmware bins: upload only if absent (immutable) -----------------------
while IFS= read -r bin; do
  name="$(basename "$bin")"
  if attachment_exists "$name"; then
    echo "skip existing bin: ${name}"
  else
    echo "upload bin: ${name}"
    upload_attachment "$bin"
  fi
done < <(find "$ASSET_DIR" -mindepth 2 -maxdepth 2 -type f -name '*.bin' | sort)

# --- Manifest: delete old then upload (overwrite) ---------------------------
OLD_ID="$(attachment_id_by_name "$MANIFEST_NAME")"
if [[ -n "$OLD_ID" ]]; then
  echo "deleting old manifest attachment id=${OLD_ID}"
  DEL_CODE="$(gitee_curl -o "$tmp_resp" -w "%{http_code}" \
    -X DELETE "${ATTACH_URL}/${OLD_ID}")"
  if [[ "$DEL_CODE" != "200" && "$DEL_CODE" != "204" ]]; then
    echo "::error::Delete old manifest failed: HTTP ${DEL_CODE}"
    jq . "$tmp_resp" 2>/dev/null || cat "$tmp_resp"
    exit 1
  fi
fi
echo "upload manifest: ${MANIFEST_NAME}"
upload_attachment "$MANIFEST_FILE"

echo "Gitee ${TAG} publish finished for ${OWNER}/${REPO} @ ${TAG}"
