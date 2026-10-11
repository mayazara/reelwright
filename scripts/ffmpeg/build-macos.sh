#!/usr/bin/env bash
# Builds LGPL shared FFmpeg for macOS with VideoToolbox.
# Needs (Homebrew): nasm pkg-config openh264 libvpx svt-av1 dav1d lame opus
set -euo pipefail
source "$(dirname "$0")/common.sh"
fetch_source
cd "${SRC}"
./configure "${COMMON_FLAGS[@]}" --enable-pthreads \
  --enable-videotoolbox --enable-audiotoolbox \
  --install-name-dir='@rpath'
make -j"$(sysctl -n hw.ncpu)"
make install
assert_lgpl
echo "FFmpeg installed to ${PREFIX}"
