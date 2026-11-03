<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/icon-dark.svg">
  <img src="assets/brand/icon.svg" width="64" alt="Tehom">
</picture>

# Tehom

Local-first, native Rust video clipping with GPUI Kit. **Under development; not a finished application or published release.**

The primary workflow is **YouTube channel URL → RSS → automatic drafts or MP4 clips**. The native sidebar has Home, Sources, Clips and Settings. Sources, jobs, drafts and settings persist locally in SQLite. Choose a local AI model once; missing models or captions stop for input. Automatic exports produce 1080×1920 H.264/AAC MP4s. Local manual video editing is also available.

This is still under development. RSS failures are shown and retried; no fake success is generated. A live RSS attempt currently returned HTTP 404 from YouTube, and no live local model was available. The tested end-to-end model/export flow uses explicitly synthetic local fixtures. See [integration evidence](docs/evidence/rss-automation.md), [the full specification](docs/master-spec.txt) and [remaining requirements](docs/status.md).

## Development

Rust 1.99.1 and platform GPUI libraries, FFmpeg/ffprobe, GStreamer and its base/good/libav plugins are needed. GPUI Kit is pinned to 0.8.1 and dependencies are recorded in Cargo.lock.

```sh
cargo run -p tehom-desktop
# Optionally open a local video immediately:
cargo run -p tehom-desktop -- /path/to/video.mp4
```

Add your channel on Home, choose automatic draft or MP4 mode, and configure your local model in Settings. Use **Video dosyası aç** for manual editing; play or seek, set start/end, then **MP4 kaydet**. A file with the same name is never replaced; choose another destination name.

On Debian/Ubuntu, the CI workflow lists the required development packages. For Windows/macOS, use the official [GStreamer runtime and development installers](https://gstreamer.freedesktop.org/download/). The [native CI workflow](.github/workflows/native-prototype.yml) records the exact setup. No one-command Tehom installer is published yet.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

Tests create real, small encoded media with FFmpeg. GStreamer preview tests decode audio/video and exercise pause, seek and worker shutdown. CI captures native-window images separately; a successful build alone is not proof of playback or packaging.

## Boundaries

There is no web application shell, SaaS, social account linking, social upload, publishing scheduler or AppImage. ASR will remain a manually initiated fallback. Remote AI will require explicit BYOK selection and consent.

AGPL-3.0-or-later. Geist fonts are distributed under the included OFL. Original brand geometry is preserved; see [asset provenance](assets/README.md). Third-party runtime distribution and installer signing remain release gates.

## Headless CLI

```sh
cargo run -p tehom-cli -- --help
tehom add https://www.youtube.com/@your-channel --mode auto-draft
tehom ai --endpoint http://127.0.0.1:11434 --model your-installed-model
tehom check
tehom run-once
tehom daemon --interval-minutes 30
tehom status
```

Use `--data /your/data/directory` to select a data directory. One engine owns a data directory at a time; close the desktop before running the daemon against the same directory. Ctrl+C cancels active daemon work. The configuration-file interface, service installer and signed distributable releases are not implemented yet.
