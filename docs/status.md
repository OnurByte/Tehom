# Requirement status

The full target remains docs/master-spec.txt. This is not a completed v1 release.

## Current evidence

- The preceding `f0a2207` CI run passed Linux, Windows x64, macOS Intel and Apple Silicon. Actual native preview screenshots were captured and inspected. This proves that earlier native build/runtime, not packaging or every new feature.
- The current local workspace runs 40 passing tests: 23 automation, 11 media, 2 native UI, 4 CLI/config/shutdown. Formatting, strict Clippy and builds are checked separately.
- The desktop now leads with a channel URL and real persistent automation. It has the four-section native sidebar, unchanged user-supplied logo, Geist fonts and Oxocarbon colors. UI tests verify the channel entry and actual decoder seek from pointer/keyboard.
- Engine fixtures exercise SQLite restart/pause/cancellation/dedup, missing model/captions, candidate validation, complete batch persistence, source retention on temporary RSS failure, actual vertical MP4 output through a local HTTP model fixture, ownership-based crash recovery and late subtitle refresh. Fixture success is not live-provider success.
- A live public-channel RSS request returned HTTP 404 from YouTube. The configured source remains saved with an explicit retryable feed error; no fake videos or exports were produced. No installed local model server was available.

See [RSS integration evidence](evidence/rss-automation.md), [sidebar screenshot](evidence/sidebar-rss-home.png) and [earlier native preview evidence](evidence/linux-prototype.md).

## Full-scope audit

| Sections | Implemented foundation | Required work still missing or unverified |
| --- | --- | --- |
| 1–5, 21 | Rust workspace, shared types/media/automation, GPUI desktop and shared CLI, Manual/Auto Draft/Auto Export/Paused records | Full milestone acceptance, final crate-boundary organization, complete app/service integration |
| 6–7 | Channel-first sidebar, real sources/jobs/drafts/settings, original SVG, Geist, dark Oxocarbon styling, native named controls | Light theme, complete keyboard/AccessKit audit, recovery-flow polish and nontechnical-user studies |
| 8 | Strict YouTube channel/handle and single-video input, local-file AI queue with original-file preservation, RSS discovery, deduplication, persisted errors, yt-dlp/caption pipeline and cancellation | Successful live channel/download run, original-file save flow, experimental Kick, richer resume/rate/disk controls |
| 9 | SRT/VTT parser, versioned user uploads through desktop/CLI, platform-caption selection and refresh, no automatic ASR | Transcript editing, language persistence and explicitly initiated ASR/model-size flow |
| 10–11 | Ollama and local OpenAI-compatible transport, chunking, validated 20–90s candidates, overlap filtering, max-five drafts and durable complete batches | Live local-model run, remote BYOK providers/consent/keyring, richer ranking/silence/scene analysis and prompt settings |
| 12–13 | Native preview, play/pause/seek/volume, portrait and three-hour-source tests, manual ranges, draft opening, no-clobber export, automatic 1080×1920 H.264/AAC and cancellation | Bundled-runtime gate, sustained memory/audio checks, drag framing, captions, output profiles, progress and complete editor E2E |
| 14–15 | SQLite WAL, migrations, source/video/draft/settings state, exclusive engine ownership, persistent mode/resume, disk preflight/download monitoring, sequential bounded worker, cancellation/retry and owned export reconciliation | Configurable storage quotas/cleanup, complete event/progress history, process-tree shutdown and remaining interruption/recovery cases |
| 16 | Shared engine through CLI add/check/read-only live status/retry/cancel/resume and periodic daemon with validated TOML configuration, SIGINT/SIGTERM drain, active-download cancellation/restart test and user-service template | Remaining TOML schema (concurrency/provider/server options), automated systemd user-service installation, active-job/process-tree lifecycle tests |
| 17 | Native CI builds/runtime screenshots | One-command installers, deb/Flatpak/dmg/exe, runtime bundling, signing/notarization, signed manifests, update/rollback and clean-install tests |
| 18 | Allowlisted source URLs, loopback AI, bounded I/O, argument-array subprocesses, atomic output and ownership checks | Independent security review, full credential/remote consent system, third-party redistribution/license audit and update verification |
| 19–20 | Current automated tests and earlier four-target native checks | Current-code cross-platform E2E, accessibility/user studies, complete release gate and a successful live demo |

No social publishing, account linking, SaaS, web application shell or AppImage will be introduced. Missing work remains in scope; passing current tests does not establish v1 completion.
