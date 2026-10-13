#!/usr/bin/env bash
# Builds the Linux downloads from target/release/reelwright:
#   Reelwright-<version>-linux-x86_64.AppImage  (runs anywhere: libraries bundled)
#   reelwright_<version>_amd64.deb              (FFmpeg bundled; the rest from the system)
# Usage: build-packages.sh <ffmpeg-prefix> <version> <out-dir>
# Needs: linuxdeploy (LINUXDEPLOY=path), dpkg-dev, patchelf.
set -euo pipefail
FF="$1"; VERSION="$2"; OUT="$3"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LINUXDEPLOY="${LINUXDEPLOY:-linuxdeploy}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# --- AppImage -------------------------------------------------------------
APPDIR="$WORK/AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/doc/reelwright"
cp "$ROOT/target/release/reelwright" "$APPDIR/usr/bin/reelwright"
"$ROOT/packaging/stage-licenses.sh" "$FF" "$APPDIR/usr/share/doc/reelwright/licenses"
cp "$ROOT/assets/branding/icon-256.png" "$WORK/reelwright.png"
libs=()
for so in "$FF"/lib/lib*.so.*; do
  [[ -L "$so" ]] || libs+=(--library "$so")
done
(
  cd "$WORK"
  LD_LIBRARY_PATH="$FF/lib" OUTPUT="$OUT/Reelwright-$VERSION-linux-x86_64.AppImage" \
    "$LINUXDEPLOY" --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/reelwright" "${libs[@]}" \
    --desktop-file "$ROOT/packaging/linux/reelwright.desktop" \
    --icon-file "$WORK/reelwright.png" \
    --output appimage
)

# --- .deb -----------------------------------------------------------------
PKG="$WORK/deb"
LIB="$PKG/usr/lib/reelwright"
mkdir -p "$LIB" "$PKG/usr/bin" "$PKG/DEBIAN" \
  "$PKG/usr/share/applications" "$PKG/usr/share/icons/hicolor/256x256/apps" \
  "$PKG/usr/share/doc/reelwright"
cp "$ROOT/target/release/reelwright" "$LIB/reelwright"
cp -a "$FF"/lib/lib*.so* "$LIB/"
# Libraries FFmpeg uses that no package provides here (built from source
# into /usr/local, e.g. OpenH264) ship beside it too.
for so in $(LD_LIBRARY_PATH="$FF/lib" ldd "$FF"/lib/lib*.so.*[0-9] | awk '$3 ~ /^\/usr\/local\// {print $3}' | sort -u); do
  cp -L "$so" "$LIB/$(basename "$so")"
done
# The app and FFmpeg find FFmpeg beside themselves.
patchelf --set-rpath '$ORIGIN' "$LIB/reelwright"
for so in "$LIB"/lib*.so.*; do
  [[ -L "$so" ]] || patchelf --set-rpath '$ORIGIN' "$so"
done
ln -s ../lib/reelwright/reelwright "$PKG/usr/bin/reelwright"
cp "$ROOT/packaging/linux/reelwright.desktop" "$PKG/usr/share/applications/"
cp "$ROOT/assets/branding/icon-256.png" "$PKG/usr/share/icons/hicolor/256x256/apps/reelwright.png"
"$ROOT/packaging/stage-licenses.sh" "$FF" "$PKG/usr/share/doc/reelwright/licenses"
# Everything but FFmpeg comes from the system: work out which packages.
mkdir -p "$WORK/shlibs/debian"
printf 'Source: reelwright\n\nPackage: reelwright\nArchitecture: amd64\n' > "$WORK/shlibs/debian/control"
DEPENDS="$(cd "$WORK/shlibs" && dpkg-shlibdeps -O -l"$LIB" "$LIB/reelwright" "$LIB"/lib*.so.*[0-9] 2>/dev/null \
  | sed -n 's/^shlibs:Depends=//p')"
# Graphics (Vulkan) and fonts for CJK/emoji text, which aren't linked.
DEPENDS="$DEPENDS, libvulkan1, mesa-vulkan-drivers | vulkan-icd"
SIZE="$(du -sk "$PKG/usr" | cut -f1)"
cat > "$PKG/DEBIAN/control" <<CTRL
Package: reelwright
Version: $VERSION
Architecture: amd64
Maintainer: Reelwright developers <noreply@reelwright.invalid>
Installed-Size: $SIZE
Depends: $DEPENDS
Recommends: fonts-noto-cjk, fonts-noto-color-emoji
Section: video
Priority: optional
Homepage: https://github.com/mayazara/reelwright
Description: Offline video editor
 Edit, record and export videos entirely on this computer: multi-track
 timeline, effects, transitions, text, graphics, screen and webcam
 recording, and export to MP4, WebM, GIF and MP3. No internet needed.
CTRL
dpkg-deb --root-owner-group --build "$PKG" "$OUT/reelwright_${VERSION}_amd64.deb"
echo "Built:"
ls -la "$OUT"
