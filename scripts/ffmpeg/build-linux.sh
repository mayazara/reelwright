#!/usr/bin/env bash
# Builds LGPL shared FFmpeg for Linux with VAAPI hardware acceleration.
# Needs: build-essential nasm pkg-config libxcb1-dev libva-dev libopenh264-dev libvpx-dev
#        libsvtav1enc-dev libdav1d-dev libmp3lame-dev libopus-dev zlib1g-dev
set -euo pipefail
source "$(dirname "$0")/common.sh"
fetch_source
cd "${SRC}"
./configure "${COMMON_FLAGS[@]}" --enable-pthreads --enable-indev=xcbgrab,v4l2 --enable-libxcb --enable-vaapi --enable-libdrm
make -j"$(nproc)"
make install
assert_lgpl
echo "FFmpeg installed to ${PREFIX}"
echo "export PKG_CONFIG_PATH=${PREFIX}/lib/pkgconfig  LD_LIBRARY_PATH=${PREFIX}/lib"
