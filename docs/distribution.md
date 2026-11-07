# Distribution

No published release, updater or signed package exists yet. `packaging/linux/build-deb.sh` builds an unsigned Debian package (`target/deb/tehom_<version>_<arch>.deb`) with both binaries, a desktop entry, the logo recoloured only (geometry verified identical), licences and example configuration. It declares FFmpeg, yt-dlp and GStreamer base/good/libav as dependencies rather than bundling them. It has been built and inspected on Arch Linux but not installed on Debian or Ubuntu. Do not copy placeholder install-domain commands into user instructions.

The native-prototype CI builds Linux x64, Windows x64 and both macOS architectures, runs real-media tests and captures native windows. Artifacts are development evidence, not signed distributable packages. Inspect screenshots and logs before recording the preview gate as passed.

Release work after the gate must provide user-scope, integrity/signature-checked installation, versioned updates and rollback; deb, Flatpak, dmg and Windows exe; runtime bundling and license notices; real clean-install/uninstall checks. AppImage and web shells remain excluded.
