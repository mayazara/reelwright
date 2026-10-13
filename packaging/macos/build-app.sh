#!/usr/bin/env bash
# Builds Reelwright.app and a DMG from target/release/reelwright.
# FFmpeg's libraries and the libraries they use go in Contents/Frameworks,
# found through @rpath (and replaceable, as the LGPL requires).
#
# Usage: build-app.sh <ffmpeg-prefix> <version> <arch> <out-dir>
# Signing: with MACOS_SIGN_IDENTITY set (a Developer ID Application
# identity in the keychain) the app is signed with the hardened runtime;
# with APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD also set, the DMG is
# notarised and stapled. Otherwise the app is signed ad hoc (needed to run
# on Apple Silicon at all) and people open it with right-click → Open.
set -euo pipefail
FF="$1"; VERSION="$2"; ARCH="$3"; OUT="$4"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="$OUT/Reelwright.app"
C="$APP/Contents"

rm -rf "$APP"
mkdir -p "$C/MacOS" "$C/Frameworks" "$C/Resources"
cp "$ROOT/target/release/reelwright" "$C/MacOS/Reelwright"

# Icon: an iconset from the 256 px PNG.
ICONSET="$OUT/Reelwright.iconset"
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
for s in 16 32 64 128 256; do
  sips -z $s $s "$ROOT/assets/branding/icon-256.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
done
for s in 16 32 64 128; do
  cp "$ICONSET/icon_$((s * 2))x$((s * 2)).png" "$ICONSET/icon_${s}x${s}@2x.png"
done
rm "$ICONSET/icon_64x64.png"
iconutil -c icns "$ICONSET" -o "$C/Resources/Reelwright.icns"
rm -rf "$ICONSET"

"$ROOT/packaging/stage-licenses.sh" "$FF" "$C/Resources/licenses"

# FFmpeg's libraries, then every non-system library they need, rewritten
# to load from Frameworks.
cp -a "$FF"/lib/*.dylib "$C/Frameworks/"
bundle_deps() {
  local file="$1" dep name
  for dep in $(otool -L "$file" | tail -n +2 | awk '{print $1}'); do
    case "$dep" in
      /usr/lib/*|/System/*|@rpath/*|@executable_path/*|@loader_path/*) continue ;;
    esac
    name="$(basename "$dep")"
    if [[ ! -f "$C/Frameworks/$name" ]]; then
      cp "$dep" "$C/Frameworks/$name"
      chmod u+w "$C/Frameworks/$name"
      install_name_tool -id "@rpath/$name" "$C/Frameworks/$name"
      bundle_deps "$C/Frameworks/$name"
    fi
    install_name_tool -change "$dep" "@rpath/$name" "$file"
  done
}
for lib in "$C"/Frameworks/*.dylib; do
  [[ -L "$lib" ]] && continue
  chmod u+w "$lib"
  install_name_tool -add_rpath "@loader_path" "$lib" 2>/dev/null || true
  bundle_deps "$lib"
done
bundle_deps "$C/MacOS/Reelwright"
install_name_tool -add_rpath "@executable_path/../Frameworks" "$C/MacOS/Reelwright" 2>/dev/null || true

# Nothing may still point outside the app.
if otool -L "$C/MacOS/Reelwright" "$C"/Frameworks/*.dylib | grep -E '^\s+/(opt|usr/local)/'; then
  echo "error: a library still loads from outside the app" >&2
  exit 1
fi

# The oldest macOS the app runs on is the newest any bundled library needs
# (Homebrew builds for the macOS it runs on).
MIN_MACOS="$(for f in "$C/MacOS/Reelwright" "$C"/Frameworks/*.dylib; do
  [[ -L "$f" ]] || otool -l "$f" | awk '/LC_BUILD_VERSION/{b=1} b&&/minos/{print $2; b=0}'
done | sort -t. -k1,1n -k2,2n | tail -1)"
echo "Minimum macOS: $MIN_MACOS"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@MIN_MACOS@/$MIN_MACOS/g" \
  "$ROOT/packaging/macos/Info.plist.in" > "$C/Info.plist"
echo "$MIN_MACOS" > "$OUT/min-macos-$ARCH.txt"

if [[ -n "${MACOS_SIGN_IDENTITY:-}" ]]; then
  SIGN=(--sign "$MACOS_SIGN_IDENTITY" --options runtime --timestamp)
else
  SIGN=(--sign -)
fi
for lib in "$C"/Frameworks/*.dylib; do
  [[ -L "$lib" ]] || codesign --force "${SIGN[@]}" "$lib"
done
codesign --force "${SIGN[@]}" --entitlements "$ROOT/packaging/macos/entitlements.plist" "$APP"
codesign --verify --deep --strict "$APP"

DMG="$OUT/Reelwright-$VERSION-macos-$ARCH.dmg"
rm -f "$DMG"
STAGE="$OUT/dmg"
rm -rf "$STAGE"; mkdir -p "$STAGE"
cp -a "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Reelwright" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

if [[ -n "${MACOS_SIGN_IDENTITY:-}" && -n "${APPLE_ID:-}" && -n "${APPLE_TEAM_ID:-}" && -n "${APPLE_APP_PASSWORD:-}" ]]; then
  codesign --force --sign "$MACOS_SIGN_IDENTITY" --timestamp "$DMG"
  xcrun notarytool submit "$DMG" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
    --password "$APPLE_APP_PASSWORD" --wait
  xcrun stapler staple "$DMG"
fi
echo "Built $DMG"
