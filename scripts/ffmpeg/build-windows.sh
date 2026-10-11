#!/usr/bin/env bash
# Builds LGPL shared FFmpeg for Windows x64 from an MSYS2 MINGW64 shell with
# D3D11VA, DXVA2, NVDEC/NVENC (ffnvcodec headers), QSV (libvpl) and AMF.
# Needs (pacman): mingw-w64-x86_64-{toolchain,nasm,pkgconf,openh264,libvpx,
#   svt-av1,dav1d,lame,opus,ffnvcodec-headers,libvpl,amf-headers}
set -euo pipefail
source "$(dirname "$0")/common.sh"
fetch_source
cd "${SRC}"
./configure "${COMMON_FLAGS[@]}" \
  --target-os=mingw32 --arch=x86_64 \
  --enable-d3d11va --enable-dxva2 \
  --enable-ffnvcodec --enable-nvdec --enable-nvenc \
  --enable-libvpl --enable-amf
make -j"$(nproc)"
make install
assert_lgpl
echo "FFmpeg installed to ${PREFIX}"
