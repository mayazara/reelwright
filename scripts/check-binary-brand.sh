#!/usr/bin/env bash
# Checks built binaries for other products' names (PLAN.md: the reference
# editor's name nowhere; its vendor's name only where code names the OS's
# own components, e.g. Windows' software renderer — OS integration).
# The words are assembled from pieces so brand-lint doesn't match this file.
# Usage: check-binary-brand.sh <files...>
set -euo pipefail
editor="clip""champ"
vendor="micro""soft"
status=0
for f in "$@"; do
  [[ -f "$f" && ! -L "$f" ]] || continue
  if grep -a -i -q "$editor" "$f"; then
    echo "error: $f contains the reference editor's name:" >&2
    grep -a -i -o ".\{0,40\}$editor.\{0,40\}" "$f" | head -5 >&2
    status=1
  fi
  os=$(grep -a -i -o "[[:print:]]\{0,30\}$vendor[[:print:]]\{0,30\}" "$f" | sort -u || true)
  if [[ -n "$os" ]]; then
    echo "note: $f names OS components:"
    echo "$os" | head -10 | sed 's/^/  /'
  fi
done
[[ $status -eq 0 ]] && echo "No other product names in $# file(s)."
exit $status
