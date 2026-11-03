# Verification status

This is the mandatory native-preview/export prototype, not a completed Tehom release. The full specification in master-spec.txt remains authoritative.

## Evidence obtained on Linux

- GPUI Kit 0.8.1 with its pinned GPUI snapshot compiles; native window displayed a decoded local H.264/AAC MP4.
- Real-media tests cover metadata/geometry, audio-only rejection, valid export, reversed intervals, existing output preservation, no visible unfinished MP4, cancellation cleanup, GStreamer decode with nontrivial row stride, pause, seek and worker shutdown.
- Video work runs outside UI callbacks. The decoder keeps one latest frame, and the UI releases old GPU images.
- Brand SVG copied byte for byte; Geist/Geist Mono embedded with OFL.

The local machine initially lacked GStreamer good/libav plugins. Development validation used package-manager-sourced plugins extracted into the task's work directory, without changing the machine's installed packages. This does not establish that a standalone release bundles its runtime correctly.

## Remaining prototype gate

- Linux native form and GTK save-dialog export verified; see [recorded evidence](evidence/linux-prototype.md). Timeline/volume/portrait/long-source spike cases remain pending.
- Windows x64, macOS Intel and Apple Silicon actual build, real-media tests and native-window image inspection through CI.
- X11 and Wayland evidence kept separate. A decoder test does not establish audible sound, GPU memory stability or native file-picker behavior on all systems.

## Full product requirements still pending

| Specification section | Required result | Status |
| --- | --- | --- |
| 1–5, 21 | Shared Rust desktop/CLI/headless engine and complete mode semantics | Initial workspace and prototype only |
| 6–7 | Four-section UX, light/dark themes, editable drafts, accessibility/AccessKit and user studies | Prototype editing form, dark styling and native controls only |
| 8 | YouTube/video/channel resolution, RSS, download/cancel/resume/deduplication, experimental Kick, original download | Not implemented |
| 9 | SRT/VTT import, platform captions, language/versioning, manual ASR | Not implemented |
| 10–11 | Local and BYOK providers, structured AI candidates, bounds/overlap/ranking validation | Not implemented |
| 12–14 | Full preview/editor behavior, crop, captions, output profiles, SQLite and file storage | Initial preview and MP4 range export only |
| 15 | Durable jobs, concurrency/disk limits, cancel/retry and crash recovery | Export cancellation only |
| 16 | Headless service, TOML, scheduling, shared CLI commands | Not implemented |
| 17 | One-command verified installation, deb/Flatpak/dmg/exe, signed manifests, update and rollback | Not implemented |
| 18 | Full security review, OS credential store, URL/DNS safeguards and consent flows | Local path subprocess arguments and overwrite/partial-file protection only |
| 19–20 | Full integration/E2E, native accessibility, nontechnical-user tests, three-platform release gates | Initial media tests and CI definition only |

CI must produce actual results before any remote platform is marked verified. Runtime/toolchain checks do not prove release packaging, signing or installation.
