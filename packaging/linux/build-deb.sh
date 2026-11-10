#!/usr/bin/env bash
# Builds a Debian package of the Tehom CLI and desktop app.
#
#   packaging/linux/build-deb.sh            # writes target/deb/tehom_<version>_<arch>.deb
#   JOBS=4 packaging/linux/build-deb.sh     # parallel rustc jobs (default 2)
#   SKIP_BUILD=1 packaging/linux/build-deb.sh  # reuse existing release binaries
#
# Requires cargo, dpkg-deb and fakeroot, plus git and CMake for whisper-cli.
# The package is unsigned; signing, update channels and clean-install tests
# are separate release steps.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
[[ -n "$version" ]] || { echo "workspace version not found" >&2; exit 1; }
case "$(uname -m)" in
    x86_64) arch=amd64 ;;
    aarch64) arch=arm64 ;;
    *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

if [[ -z "${SKIP_BUILD:-}" ]]; then
    cargo build --release -j "${JOBS:-2}" -p tehom-cli -p tehom-desktop
fi
# Speech-to-text engine beside tehom-desktop, from a pinned whisper.cpp.
[[ -x target/release/whisper-cli ]] || packaging/build-whisper.sh target/release
for binary in tehom tehom-desktop whisper-cli; do
    [[ -x "target/release/$binary" ]] || { echo "missing target/release/$binary" >&2; exit 1; }
done

stage="target/deb/tehom_${version}_${arch}"
rm -rf "$stage"
install -Dm755 target/release/tehom "$stage/usr/bin/tehom"
install -Dm755 target/release/tehom-desktop "$stage/usr/bin/tehom-desktop"
install -Dm755 target/release/whisper-cli "$stage/usr/bin/whisper-cli"

# Launcher icons from assets/brand/generate-icons.py: the unchanged mark on a
# tile (spec §7); the path geometry is checked against the source.
icon="$stage/usr/share/icons/hicolor/scalable/apps/org.tehom.Tehom.svg"
install -Dm644 assets/brand/icon-tile.svg "$icon"
if [[ "$(grep -o ' d="[^"]*"' assets/brand/icon.svg)" != "$(grep -o ' d="[^"]*"' "$icon")" ]]; then
    echo "icon geometry changed" >&2
    exit 1
fi
for png in assets/brand/hicolor/*.png; do
    size="$(basename "$png" .png)"
    install -Dm644 "$png" "$stage/usr/share/icons/hicolor/${size}x$size/apps/org.tehom.Tehom.png"
done

install -Dm644 /dev/stdin "$stage/usr/share/applications/org.tehom.Tehom.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Tehom
Comment=Create clips from videos and channels on your own computer
Exec=tehom-desktop
Icon=org.tehom.Tehom
Terminal=false
Categories=AudioVideo;Video;
StartupWMClass=org.tehom.Tehom
DESKTOP

doc="$stage/usr/share/doc/tehom"
install -Dm644 LICENSE "$doc/LICENSE"
install -Dm644 assets/fonts/OFL.txt "$doc/fonts-OFL.txt"
install -Dm644 target/release/whisper.cpp-LICENSE.txt "$doc/whisper.cpp-LICENSE"
install -Dm644 config/tehom.example.toml "$doc/examples/tehom.toml"
install -Dm644 packaging/linux/tehom.service "$doc/examples/tehom.service"
install -Dm644 /dev/stdin "$doc/copyright" <<COPYRIGHT
Tehom $version
License: AGPL-3.0-or-later (see LICENSE).
Embedded Geist and Geist Mono fonts: SIL Open Font License 1.1 (see fonts-OFL.txt).
/usr/bin/whisper-cli: whisper.cpp, MIT License (see whisper.cpp-LICENSE).
Rust dependency licenses have not yet been audited for redistribution.
COPYRIGHT

size="$(du -sk --exclude=DEBIAN "$stage" | cut -f1)"
install -Dm644 /dev/stdin "$stage/DEBIAN/control" <<CONTROL
Package: tehom
Version: $version
Architecture: $arch
Maintainer: Tehom contributors
Installed-Size: $size
Depends: libc6, ffmpeg, yt-dlp, libgstreamer1.0-0, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good, gstreamer1.0-libav
Section: video
Priority: optional
Description: Local AI clip generator for videos and channels
 Tehom turns a video, a YouTube channel or a local file into short clips on
 your own computer, using a local AI model and FFmpeg. It never publishes to
 social media services.
CONTROL

package="target/deb/tehom_${version}_${arch}.deb"
fakeroot dpkg-deb --root-owner-group -Zxz --build "$stage" "$package" >/dev/null
echo "$package"
