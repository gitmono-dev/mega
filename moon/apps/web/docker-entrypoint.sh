#!/bin/sh
# =============================================================================
# Runtime environment injection for the unified web image
# =============================================================================
# Next.js inlines NEXT_PUBLIC_* values into the client bundle at build time, so
# normally each environment needs its own image. This image is instead built
# once with placeholder URLs (see apps/web/.env.runtime). At container start we
# rewrite those placeholders inside the compiled assets with the real values
# provided as runtime environment variables, then launch the server.
#
# Each environment (ECS task definition / Cloud Run service / docker run -e ...)
# must supply the NEXT_PUBLIC_* variables it needs.
# =============================================================================
set -eu

ASSET_DIR="${NEXT_ASSET_DIR:-/app/apps/web/.next}"

# placeholder<TAB>runtime-env-var-name (keep in sync with apps/web/.env.runtime)
TAB="$(printf '\t')"
MAPPINGS="\
https://rt-api.placeholder.local${TAB}NEXT_PUBLIC_API_URL
https://rt-internal-api.placeholder.local${TAB}NEXT_PUBLIC_INTERNAL_API_URL
https://rt-mono-api.placeholder.local${TAB}NEXT_PUBLIC_MONO_API_URL
https://rt-orion-api.placeholder.local${TAB}NEXT_PUBLIC_ORION_API_URL
https://rt-auth.placeholder.local${TAB}NEXT_PUBLIC_AUTH_URL
https://rt-web.placeholder.local${TAB}NEXT_PUBLIC_WEB_URL
wss://rt-sync.placeholder.local${TAB}NEXT_PUBLIC_SYNC_URL
https://rt-crates-pro.placeholder.local${TAB}NEXT_PUBLIC_CRATES_PRO_URL"

# Escape a string for safe use on the left (regex) side of a sed s||| command.
escape_regex() { printf '%s' "$1" | sed -e 's/[.[\*^$()+?{|/\\]/\\&/g'; }
# Escape a string for safe use on the right (replacement) side of a sed command.
escape_repl() { printf '%s' "$1" | sed -e 's/[&|\\]/\\&/g'; }

if [ ! -d "$ASSET_DIR" ]; then
  echo "[entrypoint] WARN: asset dir $ASSET_DIR not found; skipping injection"
else
  echo "[entrypoint] injecting runtime environment into $ASSET_DIR"
  printf '%s\n' "$MAPPINGS" | while IFS="$TAB" read -r placeholder varname; do
    [ -z "${placeholder:-}" ] && continue
    value="$(printenv "$varname" 2>/dev/null || true)"
    if [ -z "$value" ]; then
      echo "[entrypoint] WARN: $varname not set; leaving placeholder $placeholder"
      continue
    fi
    lhs="$(escape_regex "$placeholder")"
    rhs="$(escape_repl "$value")"
    grep -rlF \
      --include='*.js' --include='*.json' --include='*.html' --include='*.css' \
      "$placeholder" "$ASSET_DIR" 2>/dev/null \
      | while IFS= read -r file; do
          sed -i "s|$lhs|$rhs|g" "$file"
        done
    echo "[entrypoint] $varname applied"
  done
fi

exec node /app/apps/web/server.js
