#!/usr/bin/env bash
# doctrine-lint — assert docs/doctrine/doctrine-manifest.yaml is the single source of truth
# and that doctrine cross-links don't rot. Pure bash + coreutils, ZERO runtime deps.
#
#   Run:  bash docs/scripts/doctrine-lint.sh    (also fired by .hooks/pre-commit and CI)
#
# Rules:
#   1. Coverage  — every docs/doctrine/*-doctrine.md is registered in the manifest.
#   2. Existence — every entry's path exists and matches docs/doctrine/<id>-doctrine.md.
#   3. Pointers  — every pointers[] file exists AND links to the doctrine (rename-rot guard).

set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
MANIFEST="$ROOT/docs/doctrine/doctrine-manifest.yaml"
DOCTRINE_DIR="$ROOT/docs/doctrine"

[ -f "$MANIFEST" ] || { echo "doctrine-lint: manifest not found at $MANIFEST" >&2; exit 1; }

errors=0
errmsgs=""
fail() { errmsgs="${errmsgs}  ✗ $1"$'\n'; errors=$((errors + 1)); }

registered=" "   # space-delimited set of registered basenames
count=0

# Validate one accumulated entry (id / path / space-separated pointers).
validate() {
  local id="$1" path="$2" pointers="$3" base p
  [ -n "$id" ] || return 0
  count=$((count + 1))
  if [ -z "$path" ]; then fail "entry '$id': missing path"; return; fi
  base=$(basename "$path")
  registered="${registered}${base} "
  [ "$base" = "$id-doctrine.md" ] || \
    fail "entry '$id': path $path should be docs/doctrine/$id-doctrine.md"
  if [ ! -f "$ROOT/$path" ]; then fail "entry '$id': path does not exist: $path"; return; fi
  for p in $pointers; do
    if [ ! -f "$ROOT/$p" ]; then fail "entry '$id': pointer file missing: $p"; continue; fi
    grep -qF -- "$base" "$ROOT/$p" || \
      fail "entry '$id': $p should link to $base but doesn't (rename-rot?)"
  done
}

cur_id="" cur_path="" cur_pointers=""
while IFS= read -r raw || [ -n "$raw" ]; do
  line="${raw%%#*}"                                   # strip comments
  line="${line%"${line##*[![:space:]]}"}"             # strip trailing whitespace
  [ -n "$line" ] || continue
  if [[ "$line" =~ ^[[:space:]]*-[[:space:]]+id:[[:space:]]*(.+)$ ]]; then
    validate "$cur_id" "$cur_path" "$cur_pointers"    # flush previous entry
    cur_id="${BASH_REMATCH[1]}"; cur_path=""; cur_pointers=""
  elif [[ "$line" =~ ^[[:space:]]+path:[[:space:]]*(.+)$ ]]; then
    cur_path="${BASH_REMATCH[1]}"
  elif [[ "$line" =~ ^[[:space:]]+pointers:[[:space:]]*\[(.*)\][[:space:]]*$ ]]; then
    cur_pointers="${BASH_REMATCH[1]//,/ }"
  fi
done < "$MANIFEST"
validate "$cur_id" "$cur_path" "$cur_pointers"        # flush last entry

# Rule 1 — coverage
for f in "$DOCTRINE_DIR"/*-doctrine.md; do
  [ -e "$f" ] || continue
  b=$(basename "$f")
  case "$registered" in
    *" $b "*) ;;
    *) fail "coverage: $b exists but is not registered in docs/doctrine/doctrine-manifest.yaml" ;;
  esac
done

if [ "$errors" -gt 0 ]; then
  echo "doctrine-lint: $errors problem(s):" >&2
  printf '%s' "$errmsgs" >&2
  exit 1
fi
echo "doctrine-lint: ok — $count doctrines registered, all paths + pointers resolve."
