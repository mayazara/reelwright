#!/usr/bin/env bash
# Shared FFmpeg settings for the per-OS build scripts.
# The configuration is LGPL-only: --enable-gpl and --enable-nonfree are never set,
# and no GPL libraries (x264, x265, rubberband, ...) are enabled.

FFMPEG_VERSION="${FFMPEG_VERSION:-8.1}"
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="${ROOT}/third_party/ffmpeg"
SRC="${WORK}/src/ffmpeg-${FFMPEG_VERSION}"
PREFIX="${PREFIX:-${WORK}/build/$(uname -s | tr '[:upper:]' '[:lower:]')-$(uname -m)}"

# What the app ships with. Every decoder, demuxer and parser stays in, so
# anything people import still opens; only what the app writes is
# selected. This keeps the shipped libraries small.
ENCODERS=(
  # H.264/HEVC: OS and hardware encoders first (royalties covered by the
  # platform/vendor), OpenH264 as a fallback, MPEG-4 as a last resort.
  h264_mf hevc_mf h264_videotoolbox hevc_videotoolbox
  h264_nvenc hevc_nvenc av1_nvenc h264_qsv hevc_qsv h264_amf hevc_amf
  libopenh264 mpeg4
  # Proxies, web formats, GIF, stills.
  mjpeg libvpx_vp8 libvpx_vp9 gif png
  # Audio.
  aac libmp3lame libopus flac pcm_s16le
)
MUXERS=(mp4 mov ipod matroska webm gif mp3 wav adts flac ogg opus)

COMMON_FLAGS=(
  --prefix="${PREFIX}"
  --enable-shared
  --disable-static
  --disable-programs
  --disable-doc
  --disable-debug
  --disable-network
  --disable-autodetect
  --disable-avdevice
  --disable-avfilter
  --enable-pic
  --enable-pthreads
  --enable-zlib
  --disable-encoders
  --disable-muxers
  --disable-protocols
  --enable-protocol=file,pipe
  --enable-encoder="$(IFS=,; echo "${ENCODERS[*]}")"
  --enable-muxer="$(IFS=,; echo "${MUXERS[*]}")"
  # Permissively licensed libraries.
  --enable-libdav1d
  --enable-libopenh264
  --enable-libvpx
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
