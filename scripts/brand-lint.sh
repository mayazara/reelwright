#!/usr/bin/env bash
# Fails if third-party brand names appear outside docs/ and PLAN.md.
# OS-integration files are exempt (PLAN.md, Definition of done): Windows
# registry keys and system paths contain the vendor's name and can't change.
# The pattern is assembled from pieces so this script does not match itself.
set -euo pipefail
cd "$(dirname "$0")/.."

pattern="clip""champ|micro""soft|flu""ent|seg""oe"

if command -v rg >/dev/null 2>&1; then
  matches=$(rg -i -n --hidden -e "$pattern" \
    --glob '!docs/**' --glob '!PLAN.md' --glob '!.git/**' --glob '!target/**' --glob '!third_party/**' \
    --glob '!Cargo.lock' --glob '!packaging/windows/**' --glob '!.github/workflows/release.yml' . || true)
else
  matches=$(grep -RniE "$pattern" . \
    --exclude-dir=docs --exclude-dir=.git --exclude-dir=target --exclude-dir=third_party --exclude-dir=windows \
    --exclude=PLAN.md --exclude=Cargo.lock --exclude=release.yml || true)
fi

if [[ -n "$matches" ]]; then
  echo "brand-lint: banned brand names found:" >&2
  echo "$matches" >&2
  exit 1
fi
echo "brand-lint: clean"
