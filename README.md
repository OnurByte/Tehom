<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/icon-dark.svg">
  <img src="assets/brand/icon.svg" width="64" alt="Tehom">
</picture>

# Tehom

Local-first, native Rust video clipping with GPUI Kit. **Under development; not a finished application or published release.**

The current prototype opens a local video, previews it with audio, pauses/seeks, selects a time range and exports H.264/AAC MP4. Exports preserve existing files, remain hidden until complete and can be cancelled. No account, telemetry or cloud service is used by this prototype.

The full target also includes video/channel discovery, transcript-first AI clipping, draft management, persistent automation, a headless service and signed installers. Those milestones remain required. See [the full specification](docs/master-spec.txt) and [verification status](docs/status.md).

## Development

Rust 1.99.1 and platform GPUI libraries, FFmpeg/ffprobe, GStreamer and its base/good/libav plugins are needed. GPUI Kit is pinned to 0.8.1 and dependencies are recorded in Cargo.lock.

```sh
cargo run -p tehom-desktop
# Optionally open a local video immediately:
cargo run -p tehom-desktop -- /path/to/video.mp4
```

Use **Dosya seç**, play or seek, set start/end in seconds, then **MP4 kaydet**. A file with the same name is never replaced; choose another destination name.

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
