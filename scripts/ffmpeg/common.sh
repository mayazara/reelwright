#!/usr/bin/env bash
# Shared FFmpeg settings for the per-OS build scripts.
# The configuration is LGPL-only: --enable-gpl and --enable-nonfree are never set,
# and no GPL libraries (x264, x265, rubberband, ...) are enabled.

FFMPEG_VERSION="${FFMPEG_VERSION:-8.0.1}"
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="${ROOT}/third_party/ffmpeg"
SRC="${WORK}/src/ffmpeg-${FFMPEG_VERSION}"
PREFIX="${PREFIX:-${WORK}/build/$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"

COMMON_FLAGS=(
  --prefix="${PREFIX}"
  --enable-shared
  --disable-static
  --disable-programs
  --disable-doc
  --disable-debug
  --disable-network
  --disable-autodetect
  --enable-pic
  --enable-pthreads
  --enable-zlib
  # Royalty-free and permissive codecs. Each is optional at runtime.
  --enable-libopenh264
  --enable-libvpx
  --enable-libsvtav1
  --enable-libdav1d
  --enable-libmp3lame
  --enable-libopus
)

fetch_source() {
  mkdir -p "${WORK}/src"
  if [[ ! -d "${SRC}" ]]; then
    curl -fsSL "${FFMPEG_URL}" -o "${WORK}/src/ffmpeg.tar.xz"
    tar -C "${WORK}/src" -xf "${WORK}/src/ffmpeg.tar.xz"
  fi
}

assert_lgpl() {
  local cfg="${PREFIX}/lib/pkgconfig/libavcodec.pc"
  # ffmpeg records its licence in the generated config; refuse anything GPL.
  if grep -q "CONFIG_GPL 1" "${SRC}/config.h"; then
    echo "error: FFmpeg was configured with GPL components" >&2
    exit 1
  fi
  [[ -f "${cfg}" ]] || { echo "error: ${cfg} missing" >&2; exit 1; }
  # Ship the exact configuration and licence next to the libraries.
  mkdir -p "${PREFIX}/share/licenses/ffmpeg"
  cp "${SRC}/COPYING.LGPLv2.1" "${PREFIX}/share/licenses/ffmpeg/"
  grep -E "^#define FFMPEG_CONFIGURATION" "${SRC}/config.h" \
    > "${PREFIX}/share/licenses/ffmpeg/CONFIGURATION.txt"
  echo "Source: ${FFMPEG_URL}" >> "${PREFIX}/share/licenses/ffmpeg/CONFIGURATION.txt"
}
