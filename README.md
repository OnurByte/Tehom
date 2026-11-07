<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/icon-dark.svg">
  <img src="assets/brand/icon.svg" width="64" alt="Tehom">
</picture>

# Tehom

Local-first, native Rust video clipping with GPUI Kit. **Under development; not a finished application or published release.**

The primary workflow is **YouTube channel URL → RSS → automatic drafts or MP4 clips**. Single YouTube videos (watch, youtu.be, shorts and live links) can also be queued through the same input. The native sidebar has Home, Sources, Clips and Settings. Sources, jobs, drafts and settings persist locally in SQLite. Choose a local AI model once; missing models or captions stop for input. Automatic exports produce 1080×1920 H.264/AAC MP4s. Local files can be queued for AI clipping with **Dosyadan klip oluştur** or edited manually. Local AI jobs read the original file in place; keep it available until processing finishes. Upload SRT/VTT from the job list when captions are missing.

This is still under development. RSS failures are shown and retried; no fake success is generated. The latest live RSS check succeeded and downloaded the newest real video; it stopped for missing platform captions. Earlier checks also observed HTTP 404, so feed failures remain retryable. A separate actual Qwen 1.5B run selected and exported three validated MP4s from authored test input; this does not establish quality on natural videos. See [local model evidence](docs/evidence/local-model.md). Synthetic fixtures also cover deterministic failure/recovery behavior. See [integration evidence](docs/evidence/rss-automation.md), [the full specification](docs/master-spec.txt) and [remaining requirements](docs/status.md).

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
tehom add https://www.youtube.com/watch?v=VIDEO_ID --mode auto-export
tehom import-video /path/to/video.mp4 --mode auto-draft
tehom revise-clip CLIP_ID --start-ms 2000 --end-ms 24000
tehom ai --endpoint http://127.0.0.1:11434 --model your-installed-model
tehom check
tehom run-once
tehom daemon --interval-minutes 30
# Or copy config/tehom.example.toml, select your model, then:
tehom daemon --config /path/to/tehom.toml
tehom status
# If a video needs captions, upload them and explicitly retry:
tehom import-transcript VIDEO_ID /path/to/subtitles.srt --language tr
tehom save-original VIDEO_ID ~/Videos   # original file, separate from clips
tehom retry VIDEO_ID
```

Use `--data /your/data/directory` to select a data directory. One writable engine owns a data directory at a time; close the desktop before running the daemon against the same directory. `tehom status` uses a read-only database connection and works while either is running. It does not initialize a missing database or apply configuration changes. Ctrl+C and Unix service-stop signals cancel active daemon work and drain the current operation. See [Linux user-service setup](docs/headless-service.md). Configuration follows the spec §16 schema (see `config/tehom.example.toml`): data.directory; automation.enabled/default_mode/check_interval_minutes/max_concurrent_downloads/max_concurrent_renders; export.directory/format/orientation; ai.provider/endpoint/model; server.enabled/listen. Values that request unimplemented behavior (non-MP4 or non-vertical output, a remote API server, non-loopback listen) are rejected rather than ignored. Relative paths resolve beside the configuration file; explicit CLI data/interval options take priority. Unknown or unsupported settings are rejected. `tehom service install --config <file> [--enable]` writes the Linux systemd user unit without overwriting existing files. Signed distributable releases are not implemented yet.

## Disk space

Before downloading, Tehom requires 1 GiB available on the media volume (16 MiB for captions alone). During download it checks once per second and cancels if less than 256 MiB remains (16 MiB for captions). MP4 preflight estimates 16 Mbit/s for the selected duration plus 256 MiB headroom on the export volume. These are conservative checks, not reservations or exact size guarantees; competing writes can still exhaust space. Existing output files are never replaced. Free space and explicitly retry a job that needs input.

The platform space query uses [fs2 available_space](https://docs.rs/fs2/0.4.3/fs2/fn.available_space.html). Configurable storage quotas and managed-cache cleanup are still pending.

Local model servers must support JSON-schema structured outputs (Ollama `format` or OpenAI-compatible `response_format`). Responses still pass independent timestamp/duration validation; an empty or invalid selection produces no clip.

Open a library clip with **Taslağı düzenle**, adjust start/end in seconds and choose **Yeni taslak kaydet** to persist a new 20–90 second draft. Previous drafts and published files remain intact. Identical ranges reuse an existing record. Saving a revision does not queue automatic rendering; **MP4 kaydet** exports the current editor selection.
