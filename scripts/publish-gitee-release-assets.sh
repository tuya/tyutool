#!/usr/bin/env bash
# Publish an explicit list of files to a Gitee Release (API v5).
#
# This script deliberately has no "skip existing" path.  Gitee is a mirror
# whose attachments may be repaired in place: every target file is uploaded
# after all attachments with the same basename have been removed.
#
# Required environment:
#   GITEE_TOKEN, GITEE_REPO (owner/repo or repo name with GITEE_USER), TAG
#   GITEE_TARGET_COMMITISH (required only when the release must be created)
# Optional environment:
#   GITEE_USER, RELEASE_NAME, RELEASE_BODY
#   PRUNE_UNLISTED=true       remove attachments not in the positional list
#   WAIT_FOR_TARGET_TAG=true  wait for TAG to appear in the Gitee mirror
#   TARGET_TAG_WAIT_SECONDS=120, TARGET_TAG_POLL_SECONDS=5
#
# Usage: publish-gitee-release-assets.sh FILE [FILE ...]
set -euo pipefail

die() {
  echo "::error::$*" >&2
  exit 1
}

[[ -n "${GITEE_TOKEN:-}" ]] || die "GITEE_TOKEN must be set"
[[ -n "${GITEE_REPO:-}" ]] || die "GITEE_REPO must be set"
[[ "$#" -gt 0 ]] || die "at least one asset file is required"

if [[ "${GITEE_REPO}" == */* ]]; then
  OWNER="${GITEE_REPO%%/*}"
  REPO="${GITEE_REPO#*/}"
  [[ -n "$OWNER" && -n "$REPO" ]] || die "GITEE_REPO must be owner/repo"
else
  [[ -n "${GITEE_USER:-}" ]] || die "GITEE_USER is required when GITEE_REPO is only the repo name"
  OWNER="$GITEE_USER"
  REPO="$GITEE_REPO"
fi

TAG="${TAG:-}"
[[ -n "$TAG" ]] || die "TAG must be set"
RELEASE_NAME="${RELEASE_NAME:-$TAG}"
RELEASE_BODY="${RELEASE_BODY:-资产由 CI workflow 自动维护。}"
PRUNE_UNLISTED="${PRUNE_UNLISTED:-false}"
WAIT_FOR_TARGET_TAG="${WAIT_FOR_TARGET_TAG:-false}"
TARGET_TAG_WAIT_SECONDS="${TARGET_TAG_WAIT_SECONDS:-120}"
TARGET_TAG_POLL_SECONDS="${TARGET_TAG_POLL_SECONDS:-5}"

[[ "$TARGET_TAG_WAIT_SECONDS" =~ ^[0-9]+$ && "$TARGET_TAG_WAIT_SECONDS" -gt 0 ]] \
  || die "TARGET_TAG_WAIT_SECONDS must be a positive integer"
[[ "$TARGET_TAG_POLL_SECONDS" =~ ^[0-9]+$ && "$TARGET_TAG_POLL_SECONDS" -gt 0 ]] \
  || die "TARGET_TAG_POLL_SECONDS must be a positive integer"

declare -a ASSETS=()
declare -A TARGET_NAMES=()
for asset in "$@"; do
  [[ -f "$asset" ]] || die "asset file not found: $asset"
  name="$(basename "$asset")"
  [[ -n "$name" && "$name" != "." && "$name" != ".." ]] || die "invalid asset filename: $asset"
  [[ -z "${TARGET_NAMES[$name]+x}" ]] || die "duplicate asset basename: $name"
  TARGET_NAMES["$name"]=1
  ASSETS+=("$asset")
done

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v jq >/dev/null 2>&1 || die "jq is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required"

enc_path() {
  python3 -c 'import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1], safe=""))' "$1"
}

tmp_get="$(mktemp)"
tmp_create="$(mktemp)"
tmp_page="$(mktemp)"
tmp_attach_list="$(mktemp)"
tmp_resp="$(mktemp)"
tmp_curl_config="$(umask 077; mktemp)"
trap 'rm -f "$tmp_get" "$tmp_create" "$tmp_page" "$tmp_attach_list" "$tmp_resp" "$tmp_curl_config"' EXIT

# Keep the token in curl's private config instead of exposing it in process
# arguments or command output.  The config is removed by the EXIT trap.
printf 'header = "Authorization: token %s"\n' "$GITEE_TOKEN" > "$tmp_curl_config"

API_BASE="https://gitee.com/api/v5"
ENC_OWNER="$(enc_path "$OWNER")"
ENC_REPO="$(enc_path "$REPO")"
ENC_TAG="$(enc_path "$TAG")"

gitee_curl() {
  curl -sS -K "$tmp_curl_config" "$@"
}

api_error() {
  echo "::error::$1 (HTTP ${2:-unknown})" >&2
  exit 1
}

tag_exists() {
  local page=1 code count
  while :; do
    code="$(gitee_curl -o "$tmp_resp" -w '%{http_code}' \
      "${API_BASE}/repos/${ENC_OWNER}/${ENC_REPO}/tags?page=${page}&per_page=100")"
    if [[ "$code" == "404" ]]; then
      return 1
    elif [[ "$code" != "200" ]]; then
      api_error "List Gitee tags failed while waiting for ${TAG}" "$code"
    fi
    jq -e 'type == "array"' "$tmp_resp" >/dev/null 2>&1 \
      || api_error "Gitee tag list was not an array" "$code"
    if jq -e --arg tag "$TAG" 'any(.[]?; .name == $tag)' "$tmp_resp" >/dev/null 2>&1; then
      return 0
    fi
    count="$(jq 'length' "$tmp_resp" 2>/dev/null || printf 0)"
    (( count < 100 )) && return 1
    ((page++))
  done
}

if [[ "$WAIT_FOR_TARGET_TAG" == "true" ]]; then
  deadline=$((SECONDS + TARGET_TAG_WAIT_SECONDS))
  until tag_exists; do
    if (( SECONDS >= deadline )); then
      die "Gitee tag ${TAG} did not appear within ${TARGET_TAG_WAIT_SECONDS}s"
    fi
    sleep "$TARGET_TAG_POLL_SECONDS"
  done
  echo "Gitee tag ${TAG} is available"
fi

# --- Resolve or create the release -----------------------------------------
REL_URL="${API_BASE}/repos/${ENC_OWNER}/${ENC_REPO}/releases/tags/${ENC_TAG}"
HTTP_CODE="$(gitee_curl -o "$tmp_get" -w '%{http_code}' "$REL_URL")"
if [[ "$HTTP_CODE" == "200" ]]; then
  RELEASE_ID="$(jq -r '.id // empty' "$tmp_get")"
  [[ -n "$RELEASE_ID" ]] || api_error "Gitee release lookup returned no id" "$HTTP_CODE"
  echo "Reusing Gitee release id=${RELEASE_ID} for tag ${TAG}"
elif [[ "$HTTP_CODE" == "404" ]]; then
  [[ -n "${GITEE_TARGET_COMMITISH:-}" ]] \
    || die "GITEE_TARGET_COMMITISH is required when creating release ${TAG}"
  CREATE_URL="${API_BASE}/repos/${ENC_OWNER}/${ENC_REPO}/releases"
  jq -n \
    --arg tag "$TAG" \
    --arg name "$RELEASE_NAME" \
    --arg body "$RELEASE_BODY" \
    --arg tc "$GITEE_TARGET_COMMITISH" \
    '{tag_name: $tag, name: $name, body: $body, target_commitish: $tc, prerelease: false}' \
    | gitee_curl -o "$tmp_create" -w '%{http_code}' \
      -X POST "$CREATE_URL" -H 'Content-Type: application/json' -d @- \
      > "$tmp_resp"
  CREATE_CODE="$(<"$tmp_resp")"
  [[ "$CREATE_CODE" == "201" || "$CREATE_CODE" == "200" ]] \
    || api_error "Create Gitee release failed" "$CREATE_CODE"
  RELEASE_ID="$(jq -r '.id // empty' "$tmp_create")"
  [[ -n "$RELEASE_ID" ]] || api_error "Create response had no release id" "$CREATE_CODE"
  echo "Created Gitee release id=${RELEASE_ID}"
else
  api_error "Get Gitee release by tag failed" "$HTTP_CODE"
fi

ATTACH_URL="${API_BASE}/repos/${ENC_OWNER}/${ENC_REPO}/releases/${RELEASE_ID}/attach_files"

refresh_attachments() {
  local page=1 code count
  printf '[]' > "$tmp_attach_list"
  while :; do
    code="$(gitee_curl -o "$tmp_page" -w '%{http_code}' \
      "${ATTACH_URL}?page=${page}&per_page=100")"
    [[ "$code" == "200" ]] || api_error "List Gitee release attachments failed" "$code"
    jq -e 'type == "array"' "$tmp_page" >/dev/null \
      || api_error "Gitee attachment list was not an array" "$code"
    jq -s '.[0] + .[1]' "$tmp_attach_list" "$tmp_page" > "${tmp_attach_list}.next"
    mv "${tmp_attach_list}.next" "$tmp_attach_list"
    count="$(jq 'length' "$tmp_page")"
    (( count < 100 )) && break
    ((page++))
  done
}

attachment_ids_by_name() {
  jq -r --arg name "$1" '.[] | select(.name == $name) | .id' "$tmp_attach_list"
}

delete_attachment() {
  local id="$1" code
  code="$(gitee_curl -o "$tmp_resp" -w '%{http_code}' \
    -X DELETE "${ATTACH_URL}/${id}")"
  [[ "$code" == "200" || "$code" == "204" ]] \
    || api_error "Delete Gitee attachment failed" "$code"
}

delete_name_attachments() {
  local name="$1" id
  while IFS= read -r id; do
    [[ -n "$id" ]] || continue
    echo "deleting existing attachment: ${name} (id=${id})"
    delete_attachment "$id"
  done < <(attachment_ids_by_name "$name")
}

upload_attachment() {
  local asset="$1" code
  code="$(gitee_curl -o "$tmp_resp" -w '%{http_code}' \
    -X POST "$ATTACH_URL" -F "file=@${asset}")"
  [[ "$code" == "201" || "$code" == "200" ]] \
    || api_error "Upload Gitee attachment failed for $(basename "$asset")" "$code"
}

refresh_attachments

if [[ "$PRUNE_UNLISTED" == "true" ]]; then
  while IFS= read -r attachment; do
    id="$(jq -r '.id' <<<"$attachment")"
    name="$(jq -r '.name' <<<"$attachment")"
    if [[ -z "${TARGET_NAMES[$name]+x}" ]]; then
      echo "pruning unlisted attachment: ${name} (id=${id})"
      delete_attachment "$id"
    fi
  done < <(jq -c '.[]' "$tmp_attach_list")
fi

for asset in "${ASSETS[@]}"; do
  name="$(basename "$asset")"
  delete_name_attachments "$name"
  echo "uploading attachment: ${name}"
  upload_attachment "$asset"
done

refresh_attachments
for asset in "${ASSETS[@]}"; do
  name="$(basename "$asset")"
  count="$(jq --arg name "$name" '[.[] | select(.name == $name)] | length' "$tmp_attach_list")"
  [[ "$count" == "1" ]] || die "Gitee attachment ${name} has ${count} copies after publish; expected exactly one"
done

echo "Gitee ${TAG} publish finished for ${OWNER}/${REPO}"
