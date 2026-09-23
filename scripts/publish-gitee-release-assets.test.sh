#!/usr/bin/env bash
# Offline tests for publish-gitee-release-assets.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="${SCRIPT_DIR}/publish-gitee-release-assets.sh"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

MOCK_BIN="${TEST_ROOT}/bin"
mkdir -p "$MOCK_BIN"

cat > "${MOCK_BIN}/curl" <<'MOCK_CURL'
#!/usr/bin/env bash
set -euo pipefail

output=''
method=GET
url=''
upload=''
while (($#)); do
  case "$1" in
    -o) output="$2"; shift 2 ;;
    -X) method="$2"; shift 2 ;;
    -F) upload="${2#file=@}"; shift 2 ;;
    -w) shift 2 ;;
    -K|-H) shift 2 ;;
    --) shift; break ;;
    http*) url="$1"; shift ;;
    *) shift ;;
  esac
done
[[ -n "$url" ]] || { echo 'mock curl: URL missing' >&2; exit 2; }

scenario="${MOCK_SCENARIO:-normal}"
state="${MOCK_STATE:?MOCK_STATE is required}"
body='{}'
code=200
if [[ "$method" == GET && "$url" == */releases/tags/* ]]; then
  if [[ "$scenario" == create-null-200 ]]; then
    body='null'
  elif [[ "$scenario" == create-invalid-200 ]]; then
    body='{}'
  elif [[ "$scenario" == create-missing-target || "$scenario" == create-no-target ]]; then
    code=404
  else
    body='{"id":123}'
  fi
elif [[ "$method" == GET && "$url" == *'/tags?'* ]]; then
  if [[ "$scenario" == tag-auth-fail ]]; then
    code=401
  elif [[ "$scenario" == wait-timeout ]]; then
    code=200
    body='[]'
  elif [[ "$scenario" == wait-paginated ]]; then
    page="${url#*page=}"
    page="${page%%&*}"
    if [[ "$page" == 1 ]]; then
      body="$(jq -n '[range(0; 100) | {name: ("other-tag-" + tostring)}]')"
    else
      body='[{"name":"asset-tag"}]'
    fi
  else
    code=200
    body='[{"name":"other-tag"},{"name":"asset-tag"}]'
  fi
elif [[ "$method" == GET && "$url" == *'/attach_files?'* ]]; then
  body="$(<"$state")"
elif [[ "$method" == POST && "$url" == */releases ]]; then
  code=201
  body='{"id":123}'
elif [[ "$method" == DELETE && "$url" == */attach_files/* ]]; then
  if [[ "$scenario" == delete-fail ]]; then
    code=500
  else
    id="${url##*/}"
    jq --arg id "$id" '[.[] | select((.id | tostring) != $id)]' "$state" > "${state}.next"
    mv "${state}.next" "$state"
    code=204
  fi
elif [[ "$method" == POST && "$url" == *'/attach_files?'* || "$method" == POST && "$url" == */attach_files ]]; then
  name="$(basename "$upload")"
  if [[ "$scenario" == upload-fail ]]; then
    code=500
  else
    id="$(jq 'map(.id | tonumber) | max // 0' "$state")"
    id=$((id + 1))
    jq --arg name "$name" --argjson id "$id" '. + [{id: $id, name: $name}]' "$state" > "${state}.next"
    mv "${state}.next" "$state"
    code=201
  fi
fi
if [[ -n "$output" ]]; then printf '%s' "$body" > "$output"; fi
printf '%s' "$code"
MOCK_CURL
chmod +x "${MOCK_BIN}/curl"

make_asset() {
  local name="$1"
  printf '%s' "$name" > "${TEST_ROOT}/${name}"
}

run_publish() {
  PATH="${MOCK_BIN}:$PATH" \
    GITEE_TOKEN='test-token' \
    GITEE_REPO='owner/repo' \
    TAG='asset-tag' \
    GITEE_TARGET_COMMITISH='refactor/v3' \
    MOCK_STATE="$1" \
    MOCK_SCENARIO="${2:-normal}" \
    bash "$SCRIPT" "${@:3}"
}

make_asset a.bin
make_asset b.bin
make_asset keep.bin

test_existing_and_duplicate_attachments_are_replaced() {
  local state="${TEST_ROOT}/existing.json"
  printf '[{"id":1,"name":"a.bin"},{"id":2,"name":"a.bin"},{"id":3,"name":"old.bin"}]' > "$state"
  run_publish "$state" normal "${TEST_ROOT}/a.bin" "${TEST_ROOT}/b.bin"
  [[ "$(jq --arg n a.bin '[.[] | select(.name == $n)] | length' "$state")" == 1 ]]
  [[ "$(jq --arg n b.bin '[.[] | select(.name == $n)] | length' "$state")" == 1 ]]
  [[ "$(jq --arg n old.bin '[.[] | select(.name == $n)] | length' "$state")" == 1 ]]
}

test_missing_attachment_is_uploaded() {
  local state="${TEST_ROOT}/missing.json"
  printf '[]' > "$state"
  run_publish "$state" normal "${TEST_ROOT}/a.bin"
  [[ "$(jq --arg n a.bin '[.[] | select(.name == $n)] | length' "$state")" == 1 ]]
}

test_prune_removes_unlisted_attachments() {
  local state="${TEST_ROOT}/prune.json"
  printf '[{"id":1,"name":"keep.bin"},{"id":2,"name":"remove.bin"}]' > "$state"
  PATH="${MOCK_BIN}:$PATH" GITEE_TOKEN=test-token GITEE_REPO=owner/repo TAG=asset-tag \
    GITEE_TARGET_COMMITISH=refactor/v3 PRUNE_UNLISTED=true MOCK_STATE="$state" \
    bash "$SCRIPT" "${TEST_ROOT}/keep.bin" 2>/dev/null
  [[ "$(jq --arg n remove.bin '[.[] | select(.name == $n)] | length' "$state")" == 0 ]]
}

test_delete_failure_aborts() {
  local state="${TEST_ROOT}/delete-fail.json"
  printf '[{"id":1,"name":"a.bin"}]' > "$state"
  if run_publish "$state" delete-fail "${TEST_ROOT}/a.bin" >/dev/null 2>&1; then
    return 1
  fi
}

test_upload_failure_aborts() {
  local state="${TEST_ROOT}/upload-fail.json"
  printf '[]' > "$state"
  if run_publish "$state" upload-fail "${TEST_ROOT}/a.bin" >/dev/null 2>&1; then
    return 1
  fi
}

test_create_requires_target_commitish() {
  local state="${TEST_ROOT}/create-no-target.json"
  printf '[]' > "$state"
  if PATH="${MOCK_BIN}:$PATH" GITEE_TOKEN=test-token GITEE_REPO=owner/repo TAG=asset-tag \
      MOCK_STATE="$state" MOCK_SCENARIO=create-no-target bash "$SCRIPT" "${TEST_ROOT}/a.bin" \
      >/dev/null 2>&1; then
    return 1
  fi
}

test_create_when_missing_release_is_null_200() {
  local state="${TEST_ROOT}/create-null-200.json"
  printf '[]' > "$state"
  PATH="${MOCK_BIN}:$PATH" GITEE_TOKEN=test-token GITEE_REPO=owner/repo TAG=asset-tag \
    GITEE_TARGET_COMMITISH=refactor/v3 MOCK_STATE="$state" MOCK_SCENARIO=create-null-200 \
    bash "$SCRIPT" "${TEST_ROOT}/a.bin" >/dev/null
  [[ "$(jq --arg n a.bin '[.[] | select(.name == $n)] | length' "$state")" == 1 ]]
}

test_invalid_release_lookup_response_aborts() {
  local state="${TEST_ROOT}/create-invalid-200.json"
  printf '[]' > "$state"
  if run_publish "$state" create-invalid-200 "${TEST_ROOT}/a.bin" >/dev/null 2>&1; then
    return 1
  fi
  [[ "$(jq 'length' "$state")" == 0 ]]
}

test_wait_uses_exact_tag_from_paginated_list() {
  local state="${TEST_ROOT}/wait.json"
  printf '[]' > "$state"
  PATH="${MOCK_BIN}:$PATH" GITEE_TOKEN=test-token GITEE_REPO=owner/repo TAG=asset-tag \
    GITEE_TARGET_COMMITISH=asset-tag WAIT_FOR_TARGET_TAG=true TARGET_TAG_WAIT_SECONDS=1 \
    TARGET_TAG_POLL_SECONDS=1 MOCK_SCENARIO=wait-paginated MOCK_STATE="$state" bash "$SCRIPT" "${TEST_ROOT}/a.bin" \
    >/dev/null 2>&1
}

test_tag_auth_failure_is_immediate() {
  local state="${TEST_ROOT}/tag-auth-fail.json" output
  printf '[]' > "$state"
  output="$(PATH="${MOCK_BIN}:$PATH" GITEE_TOKEN=test-token GITEE_REPO=owner/repo TAG=asset-tag \
    GITEE_TARGET_COMMITISH=asset-tag WAIT_FOR_TARGET_TAG=true TARGET_TAG_WAIT_SECONDS=10 \
    TARGET_TAG_POLL_SECONDS=10 MOCK_SCENARIO=tag-auth-fail MOCK_STATE="$state" \
    bash "$SCRIPT" "${TEST_ROOT}/a.bin" 2>&1 || true)"
  [[ "$output" == *"HTTP 401"* ]]
}

test_existing_and_duplicate_attachments_are_replaced
test_missing_attachment_is_uploaded
test_prune_removes_unlisted_attachments
test_delete_failure_aborts
test_upload_failure_aborts
test_create_requires_target_commitish
test_create_when_missing_release_is_null_200
test_invalid_release_lookup_response_aborts
test_wait_uses_exact_tag_from_paginated_list
test_tag_auth_failure_is_immediate
echo 'publish-gitee-release-assets mock tests passed'
