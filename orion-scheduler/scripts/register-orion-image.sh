#!/usr/bin/env bash
# Register an already-uploaded Orion qcow2 in the mono image catalog.
#
# Usage (from repo root or this directory):
#   ORION_IMAGE_REGISTER_TOKEN=<admin Bearer> \
#     bash scripts/register-orion-image.sh [digest_hex]
#
# Defaults: latest published flat image under ~/.local/share/qlean/images/
# Sources scripts/.env the same way as build-custom-image.sh.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${ORION_IMAGE_ENV_FILE:-$SCRIPT_DIR/.env}"

# Source .env without clobbering variables already set in the process environment.
if [ -f "$ENV_FILE" ]; then
  _had_url=0 _had_token=0
  [ -n "${ORION_IMAGE_REGISTER_URL+x}" ] && { _had_url=1; _s_url="$ORION_IMAGE_REGISTER_URL"; }
  [ -n "${ORION_IMAGE_REGISTER_TOKEN+x}" ] && { _had_token=1; _s_token="$ORION_IMAGE_REGISTER_TOKEN"; }
  set -a
  # shellcheck disable=SC1090
  . "$ENV_FILE"
  set +a
  [ "$_had_url" = 1 ] && ORION_IMAGE_REGISTER_URL="$_s_url"
  [ "$_had_token" = 1 ] && ORION_IMAGE_REGISTER_TOKEN="$_s_token"
  unset _had_url _had_token _s_url _s_token
fi

ORION_IMAGE_REGISTER_URL="${ORION_IMAGE_REGISTER_URL:-https://git.rk8s.xuanwu.openatom.cn/api/v1/orion/images}"
IMAGE_NAME="${IMAGE_NAME:-debian-13-buck2}"

if [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != "root" ]; then
  QLEAN_HOME="$(getent passwd "$SUDO_USER" | cut -d: -f6)"
else
  QLEAN_HOME="${HOME}"
fi
OUTPUT_DIR="${OUTPUT_DIR:-$QLEAN_HOME/.local/share/qlean/images}"
IMAGE_FILE="${IMAGE_FILE:-$OUTPUT_DIR/${IMAGE_NAME}.qcow2}"
INFO_FILE="${INFO_FILE:-$OUTPUT_DIR/${IMAGE_NAME}.image-info.json}"

if [ -z "${ORION_IMAGE_REGISTER_TOKEN:-}" ]; then
  echo "ORION_IMAGE_REGISTER_TOKEN is required (admin Bearer)" >&2
  exit 1
fi
if [ ! -f "$IMAGE_FILE" ]; then
  echo "image not found: $IMAGE_FILE" >&2
  exit 1
fi

DIGEST_HEX="${1:-}"
if [ -z "$DIGEST_HEX" ]; then
  DIGEST_HEX=$(sha256sum "$IMAGE_FILE" | awk '{print $1}')
fi
OBJECT_KEY="${DIGEST_HEX}/${IMAGE_NAME}.qcow2"
INFO_KEY="${DIGEST_HEX}/image-info.json"
SIZE_BYTES=$(stat -c%s "$IMAGE_FILE")

built_at=$(jq -r '.built_at // empty' "$INFO_FILE" 2>/dev/null || true)
rust_ver=$(jq -r '.rust // empty' "$INFO_FILE" 2>/dev/null || true)
buck2_ver=$(jq -r '.buck2 // empty' "$INFO_FILE" 2>/dev/null || true)
python_ver=$(jq -r '.python // empty' "$INFO_FILE" 2>/dev/null || true)
kernel_ver=$(jq -r '.kernel // empty' "$INFO_FILE" 2>/dev/null || true)

body=$(jq -n \
  --arg digest "sha256:${DIGEST_HEX}" \
  --arg object_key "$OBJECT_KEY" \
  --arg info_object_key "$INFO_KEY" \
  --arg image_name "$IMAGE_NAME" \
  --arg built_at "$built_at" \
  --arg rust "$rust_ver" \
  --arg buck2 "$buck2_ver" \
  --arg python "$python_ver" \
  --arg kernel "$kernel_ver" \
  --argjson size_bytes "$SIZE_BYTES" \
  '{
    digest: $digest,
    object_key: $object_key,
    info_object_key: $info_object_key,
    image_name: $image_name,
    built_at: (if $built_at == "" then null else $built_at end),
    rust: (if $rust == "" then null else $rust end),
    buck2: (if $buck2 == "" then null else $buck2 end),
    python: (if $python == "" then null else $python end),
    kernel: (if $kernel == "" then null else $kernel end),
    size_bytes: $size_bytes
  }')

echo "POST $ORION_IMAGE_REGISTER_URL"
echo "digest=sha256:$DIGEST_HEX object_key=$OBJECT_KEY"
tmp=$(mktemp)
code=$(curl -sS -o "$tmp" -w '%{http_code}' -X POST "$ORION_IMAGE_REGISTER_URL" \
  -H "Authorization: Bearer ${ORION_IMAGE_REGISTER_TOKEN}" \
  -H "Content-Type: application/json" \
  -d "$body" || true)
echo "HTTP $code"
cat "$tmp"
echo
rm -f "$tmp"
[ "$code" = "200" ]
