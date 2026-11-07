#!/bin/sh
# Tehom installer for Linux and macOS. The same command on both:
#
#   curl -fsSL https://raw.githubusercontent.com/OnurByte/Tehom/main/install.sh | sh
#
# Installs `tehom` and `tehom-desktop` for the current user only (no sudo) into
# ~/.local/bin, after checking the release archive's SHA-256. Missing FFmpeg,
# yt-dlp or GStreamer are listed, and installed only if you answer yes.
#
# Options (environment variables):
#   TEHOM_VERSION=v0.1.0   install a specific release instead of the latest
#   TEHOM_BIN_DIR=DIR      install somewhere other than ~/.local/bin
#   TEHOM_YES=1            install missing tools without asking
#   TEHOM_NO_DEPS=1        never install system tools, only report them
#   TEHOM_ARCHIVE=FILE     install from a local archive (FILE.sha256 next to it)
set -eu

REPO="${TEHOM_REPO:-OnurByte/Tehom}"
VERSION="${TEHOM_VERSION:-latest}"
BIN_DIR="${TEHOM_BIN_DIR:-$HOME/.local/bin}"
GST_VERSION="1.28.8"

say() { printf '%s\n' "$*"; }
fail() {
    printf 'Tehom install stopped: %s\n' "$*" >&2
    exit 1
}
have() { command -v "$1" >/dev/null 2>&1; }

case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) fail "this system is not supported ($(uname -s)). On Windows, in PowerShell: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) fail "unsupported processor architecture: $(uname -m)" ;;
esac

asset="tehom-$os-$arch.tar.gz"
if [ "$VERSION" = latest ]; then
    base="https://github.com/$REPO/releases/latest/download"
else
    base="https://github.com/$REPO/releases/download/$VERSION"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

fetch() {
    if have curl; then
        curl -fsSL --retry 5 --retry-all-errors "$1" -o "$2"
    elif have wget; then
        wget -q "$1" -O "$2"
    else
        fail "curl or wget is required"
    fi
}

say "Downloading Tehom ($os, $arch)…"
if [ -n "${TEHOM_ARCHIVE:-}" ]; then
    cp "$TEHOM_ARCHIVE" "$tmp/$asset"
    cp "$TEHOM_ARCHIVE.sha256" "$tmp/$asset.sha256"
else
    fetch "$base/$asset" "$tmp/$asset" ||
        fail "could not download $asset. There may be no release for this platform yet: https://github.com/$REPO/releases"
    fetch "$base/$asset.sha256" "$tmp/$asset.sha256" || fail "could not download the checksum file"
fi

expected="$(cut -d ' ' -f 1 <"$tmp/$asset.sha256")"
if have sha256sum; then
    actual="$(sha256sum "$tmp/$asset" | cut -d ' ' -f 1)"
else
    actual="$(shasum -a 256 "$tmp/$asset" | cut -d ' ' -f 1)"
fi
[ -n "$expected" ] && [ "$expected" = "$actual" ] ||
    fail "SHA-256 mismatch; the download is damaged or altered. Nothing was installed"

tar -xzf "$tmp/$asset" -C "$tmp"
package="$tmp/tehom"
for binary in tehom tehom-desktop; do
    [ -f "$package/$binary" ] || fail "$binary is missing from the archive"
done

mkdir -p "$BIN_DIR"
for binary in tehom tehom-desktop; do
    # Copy beside the target, then rename: a running old copy keeps working.
    cp "$package/$binary" "$BIN_DIR/.$binary.new"
    chmod 755 "$BIN_DIR/.$binary.new"
    mv -f "$BIN_DIR/.$binary.new" "$BIN_DIR/$binary"
done

if [ "$os" = linux ]; then
    data="${XDG_DATA_HOME:-$HOME/.local/share}"
    mkdir -p "$data/applications" "$data/icons/hicolor/scalable/apps"
    cp "$package/org.tehom.Tehom.svg" "$data/icons/hicolor/scalable/apps/org.tehom.Tehom.svg"
    cat >"$data/applications/org.tehom.Tehom.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Tehom
Comment=Make clips from videos and channels
Exec="$BIN_DIR/tehom-desktop"
Icon=org.tehom.Tehom
Terminal=false
Categories=AudioVideo;Video;
DESKTOP
    have update-desktop-database && update-desktop-database "$data/applications" >/dev/null 2>&1 || true
else
    app="$HOME/Applications/Tehom.app/Contents"
    mkdir -p "$app/MacOS"
    cp "$BIN_DIR/tehom-desktop" "$app/MacOS/tehom-desktop"
    cat >"$app/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Tehom</string>
<key>CFBundleIdentifier</key><string>org.tehom.Tehom</string>
<key>CFBundleExecutable</key><string>tehom-desktop</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
</dict></plist>
PLIST
fi

# --- Tools Tehom runs: FFmpeg, yt-dlp, GStreamer ---------------------------
missing=""
for tool in ffmpeg ffprobe yt-dlp; do
    have "$tool" || missing="$missing $tool"
done
if [ "$os" = macos ]; then
    [ -d /Library/Frameworks/GStreamer.framework ] || missing="$missing gstreamer"
elif ! have gst-inspect-1.0 || ! gst-inspect-1.0 avdec_h264 >/dev/null 2>&1 ||
    ! gst-inspect-1.0 autoaudiosink >/dev/null 2>&1; then
    missing="$missing gstreamer"
fi

command=""
if [ -n "$missing" ]; then
    if [ "$os" = macos ]; then
        have brew || fail "Homebrew is required: https://brew.sh, then run this command again"
        pkg="gstreamer-1.0-$GST_VERSION-universal.pkg"
        command="brew install ffmpeg yt-dlp && curl -fL --retry 5 --retry-all-errors https://gstreamer.freedesktop.org/data/pkg/osx/$GST_VERSION/$pkg -o /tmp/$pkg && sudo installer -pkg /tmp/$pkg -target /"
    elif have apt-get; then
        command="sudo apt-get install -y ffmpeg yt-dlp gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-libav"
    elif have pacman; then
        command="sudo pacman -S --needed ffmpeg yt-dlp gst-plugins-base gst-plugins-good gst-libav"
    elif have dnf; then
        command="sudo dnf install -y ffmpeg yt-dlp gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-libav"
    elif have zypper; then
        command="sudo zypper install -y ffmpeg yt-dlp gstreamer-plugins-base gstreamer-plugins-good gstreamer-plugins-libav"
    fi
    say ""
    say "Missing tools:$missing"
    if [ -z "$command" ]; then
        say "Package manager not recognised; install FFmpeg, yt-dlp and GStreamer (base, good, libav)."
    elif [ -n "${TEHOM_NO_DEPS:-}" ]; then
        say "To install them: $command"
    else
        answer=n
        if [ -n "${TEHOM_YES:-}" ]; then
            answer=y
        elif [ -r /dev/tty ]; then
            say "They will be installed with:"
            say "  $command"
            printf 'Install them now? [Y/n] '
            read -r answer </dev/tty || answer=n
            [ -z "$answer" ] && answer=y
        fi
        case "$answer" in
            [YyEe]*) sh -c "$command" || say "The tools could not be installed; you can run the command yourself." ;;
            *) say "To install them later: $command" ;;
        esac
    fi
fi

say ""
say "Installed $("$BIN_DIR/tehom" --version) in $BIN_DIR"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) say "Note: $BIN_DIR is not on your PATH. Add this to your shell profile: export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac
if [ "$os" = linux ]; then
    say "Open \"Tehom\" from your app menu, or run tehom-desktop"
else
    say "Open ~/Applications/Tehom.app, or run tehom-desktop"
fi
say "For AI clips, add your own API key in Settings, or run a local model such as Ollama."
