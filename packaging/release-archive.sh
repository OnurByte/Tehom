#!/usr/bin/env bash
# Packs release binaries into the archive that install.sh / install.ps1 expect:
#   target/dist/tehom-<os>-<arch>.tar.gz (or .zip on Windows) + .sha256
# Usage: packaging/release-archive.sh [binary-directory]   (default target/release)
# The directory must also hold whisper-cli and whisper.cpp-LICENSE.txt.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
binaries="${1:-target/release}"

case "$(uname -s)" in
    Linux) os=linux ext="" ;;
    Darwin) os=macos ext="" ;;
    MINGW* | MSYS* | CYGWIN*) os=windows ext=".exe" ;;
    *) echo "unsupported OS" >&2; exit 1 ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) echo "unsupported arch" >&2; exit 1 ;;
esac

stage="target/dist/stage/tehom"
rm -rf target/dist/stage
mkdir -p "$stage"
# whisper-cli (built by the release workflow from a pinned whisper.cpp) is
# required: an archive without it would break offline transcription.
for binary in tehom tehom-desktop whisper-cli; do
    cp "$binaries/$binary$ext" "$stage/$binary$ext"
done
cp "$binaries/whisper.cpp-LICENSE.txt" "$stage/whisper.cpp-LICENSE.txt"
cp LICENSE "$stage/LICENSE"
cp assets/fonts/OFL.txt "$stage/fonts-OFL.txt"
# App icons from assets/brand/generate-icons.py: the unchanged mark on a tile
# (spec §7); the path geometry is checked against the source.
cp assets/brand/icon-tile.svg "$stage/org.tehom.Tehom.svg"
[[ "$(grep -o ' d="[^"]*"' assets/brand/icon.svg)" == "$(grep -o ' d="[^"]*"' "$stage/org.tehom.Tehom.svg")" ]] || {
    echo "icon geometry changed" >&2
    exit 1
}
case "$os" in
    linux) cp -R assets/brand/hicolor "$stage/hicolor" ;;
    macos) cp assets/brand/icon.icns "$stage/Tehom.icns" ;;
esac

# Windows has no system GStreamer, so the zip carries the part Tehom uses.
# Plug-ins are listed by hand: the full runtime is 500 MB and includes GPL ones.
gst_plugins=(
    app audioconvert audioparsers audioresample autodetect coreelements d3d11
    deinterlace directsound id3demux isomp4 libav matroska opus opusparse pbtypes
    playback typefindfunctions videoconvertscale videoparsersbad volume vorbis vpx
    wasapi wasapi2
)
bundle_imports() {
    local name imports
    imports="$(llvm-readobj --coff-imports "$1" | tr -d '\r')"
    for name in $(sed -n 's/^ *Name: \(.*\.dll\)$/\1/Ip' <<<"$imports"); do
        if [[ -f "$gst/bin/$name" && ! -e "$stage/$name" ]]; then
            cp "$gst/bin/$name" "$stage/"
            bundle_imports "$stage/$name"
        fi
    done
}
if [[ "$os" == windows ]]; then
    gst="$(cygpath -u "${GSTREAMER_1_0_ROOT_MSVC_X86_64:?GStreamer is required to pack for Windows}")"
    mkdir "$stage/gstreamer-plugins"
    cp "$gst/libexec/gstreamer-1.0/gst-plugin-scanner.exe" "$stage/"
    bundle_imports "$stage/gst-plugin-scanner.exe"
    bundle_imports "$stage/tehom-desktop.exe"
    for plugin in "${gst_plugins[@]}"; do
        cp "$gst/lib/gstreamer-1.0/gst$plugin.dll" "$stage/gstreamer-plugins/"
        bundle_imports "$stage/gstreamer-plugins/gst$plugin.dll"
    done
    # AV1 comes from the Rust plug-ins, which not every GStreamer build includes.
    if [[ -f "$gst/lib/gstreamer-1.0/gstdav1d.dll" ]]; then
        cp "$gst/lib/gstreamer-1.0/gstdav1d.dll" "$stage/gstreamer-plugins/"
        bundle_imports "$stage/gstreamer-plugins/gstdav1d.dll"
    fi
    # Rust and GStreamer need these; a clean Windows does not always have them.
    for runtime in vcruntime140 vcruntime140_1 msvcp140; do
        cp "$(cygpath -u "$SYSTEMROOT")/System32/$runtime.dll" "$stage/"
    done
    cat >"$stage/THIRD-PARTY-NOTICES.txt" <<'EOF'
This package includes unmodified binaries of GStreamer 1.28.8 and the libraries
it needs (GLib, FFmpeg through gst-libav and others), under the GNU LGPL 2.1 or
later or more permissive licences. Their source code and licence texts are at
https://gstreamer.freedesktop.org/src/ and in the Cerbero recipes at
https://gitlab.freedesktop.org/gstreamer/cerbero. The DLLs can be replaced with
compatible builds. It also includes the Microsoft Visual C++ Runtime, which
Microsoft permits to be redistributed with applications.
EOF
fi

name="tehom-$os-$arch"
cd target/dist
if [[ "$os" == windows ]]; then
    rm -f "$name.zip"
    (cd stage && 7z a -tzip "../$name.zip" tehom >/dev/null)
    archive="$name.zip"
else
    tar -czf "$name.tar.gz" -C stage tehom
    archive="$name.tar.gz"
fi
if command -v sha256sum >/dev/null; then
    sha256sum "$archive" >"$archive.sha256"
else
    shasum -a 256 "$archive" >"$archive.sha256"
fi
rm -rf stage
echo "target/dist/$archive"
