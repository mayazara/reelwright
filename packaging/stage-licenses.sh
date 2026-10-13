#!/usr/bin/env bash
# Copies every licence notice the app must ship into one folder: the app's
# own licences, bundled fonts and stickers, FFmpeg's LGPL text and build
# configuration, and the Rust crates' notices (THIRD_PARTY_LICENSES.html).
# Usage: stage-licenses.sh <ffmpeg-prefix> <dest-dir>
set -euo pipefail
FF="$1"
DEST="$2"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

mkdir -p "$DEST/fonts" "$DEST/FFmpeg"
cp "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$DEST/"
cp "$ROOT/assets/fonts/ui/Inter-OFL.txt" "$DEST/"
for f in "$ROOT"/assets/fonts/title/*/OFL.txt; do
  cp "$f" "$DEST/fonts/$(basename "$(dirname "$f")")-OFL.txt"
done
cp "$ROOT/assets/stickers/openmoji/LICENSE.txt" "$DEST/OpenMoji-CC-BY-SA-4.0.txt"
cp "$ROOT/assets/ATTRIBUTION.md" "$DEST/"
if [[ -f "$ROOT/THIRD_PARTY_LICENSES.html" ]]; then
  cp "$ROOT/THIRD_PARTY_LICENSES.html" "$DEST/"
fi
cp "$FF"/share/licenses/ffmpeg/* "$DEST/FFmpeg/" 2>/dev/null || true
cat > "$DEST/FFmpeg/SOURCE.txt" <<'TXT'
FFmpeg is used under the GNU Lesser General Public License v2.1 (LGPL build:
no GPL or non-free components). Its shared libraries ship as separate files
and can be replaced with any compatible FFmpeg build of the same major
version.

Built from the official source release listed in CONFIGURATION.txt
(https://ffmpeg.org/releases/) by the scripts in scripts/ffmpeg/ of the
Reelwright source repository, with the configure options shown there.
TXT
echo "Licences staged in $DEST"
