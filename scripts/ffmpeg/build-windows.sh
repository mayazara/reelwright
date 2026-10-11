#!/usr/bin/env bash
# Builds the trimmed LGPL FFmpeg shipped with the Windows app.
#
# Run from an MSYS2 UCRT64 shell (the UCRT C runtime is part of Windows 10
# and 11, so nothing extra needs installing). Needs:
#   pacman -S make diffutils mingw-w64-ucrt-x86_64-{gcc,nasm,pkgconf,zlib,
#     dav1d,openh264,libvpx,lame,opus,ffnvcodec-headers,amf-headers,libvpl}
#
# The supporting libraries are linked statically into the FFmpeg DLLs, so
# the result is just avcodec/avformat/avutil/swscale/swresample DLLs that
# depend only on Windows system DLLs and each other. The script checks this.
set -euo pipefail
source "$(dirname "$0")/common.sh"

# Make the linker pick static archives for the third-party libraries by
# hiding their import libraries for the duration of the build.
HIDDEN=()
for lib in dav1d openh264 vpx mp3lame opus z vpl; do
  for f in "${MINGW_PREFIX}/lib/lib${lib}.dll.a"; do
    if [[ -f "$f" ]]; then
      mv "$f" "$f.hidden"
      HIDDEN+=("$f")
    fi
  done
done
restore() { for f in "${HIDDEN[@]}"; do mv "$f.hidden" "$f"; done; }
trap restore EXIT

fetch_source
cd "${SRC}"
make distclean >/dev/null 2>&1 || true
./configure "${COMMON_FLAGS[@]}" \
  --target-os=mingw32 --arch=x86_64 \
  --pkg-config-flags=--static \
  --extra-ldflags="-static -static-libgcc -static-libstdc++" \
  --extra-libs="-lstdc++ -lwinpthread" \
  --enable-mediafoundation \
  --enable-d3d11va --enable-dxva2 \
  --enable-ffnvcodec --enable-nvenc --enable-nvdec \
  --enable-amf --enable-libvpl
make -j"$(nproc)"
make install
assert_lgpl

# Only Windows system DLLs and the FFmpeg DLLs themselves may be needed.
allowed='^(kernel32|user32|gdi32|advapi32|ole32|oleaut32|shell32|ws2_32|bcrypt|secur32|ncrypt|crypt32|mfplat|mf|mfuuid|propsys|d3d11|dxgi|dxva2|evr|strmiids|psapi|shlwapi|msvcrt|api-ms-win-[a-z0-9-]+|ucrtbase|ntdll|avcodec-[0-9]+|avformat-[0-9]+|avutil-[0-9]+|swscale-[0-9]+|swresample-[0-9]+)\.dll$'
bad=0
for dll in "${PREFIX}"/bin/*.dll; do
  while read -r dep; do
    if ! echo "$dep" | tr '[:upper:]' '[:lower:]' | grep -Eq "$allowed"; then
      echo "error: $(basename "$dll") needs $dep" >&2
      bad=1
    fi
  done < <(objdump -p "$dll" | awk '/DLL Name:/ {print $3}')
done
[[ $bad -eq 0 ]] || exit 1

echo "FFmpeg ${FFMPEG_VERSION} installed to ${PREFIX}"
ls -la "${PREFIX}/bin"
