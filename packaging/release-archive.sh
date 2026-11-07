#!/usr/bin/env bash
# Packs release binaries into the archive that install.sh / install.ps1 expect:
#   target/dist/tehom-<os>-<arch>.tar.gz (or .zip on Windows) + .sha256
# Usage: packaging/release-archive.sh [binary-directory]   (default target/release)
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
for binary in tehom tehom-desktop; do
    cp "$binaries/$binary$ext" "$stage/$binary$ext"
done
cp LICENSE "$stage/LICENSE"
cp assets/fonts/OFL.txt "$stage/fonts-OFL.txt"
# Launcher icon: only currentColor is replaced (spec §7); geometry is checked.
sed 's/currentColor/#be95ff/g' assets/brand/icon.svg >"$stage/org.tehom.Tehom.svg"
[[ "$(grep -o ' d="[^"]*"' assets/brand/icon.svg)" == "$(grep -o ' d="[^"]*"' "$stage/org.tehom.Tehom.svg")" ]] || {
    echo "icon geometry changed" >&2
    exit 1
}

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
