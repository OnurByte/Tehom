# RSS automation integration evidence

The user reprioritized the real channel/RSS workflow and requested a sidebar following the master design. The application now opens on channel entry, with native Home/Sources/Clips/Settings navigation and the explicitly supplied logo(2).svg. The supplied SVG was byte-identical to the existing original; geometry remains unchanged.

The shared tehom-automation engine persists sources, videos, candidates, export intents and settings in SQLite WAL. First channel discovery seeds the feed and queues only its newest video; later discoveries queue new entries. Automatic draft and export modes, persistent pause/resume, retry and cancellation are wired into the desktop. A standard file lock prevents two engines from recovering the same active work. A CLI uses the same engine for add/check/status/daemon commands.

Local workspace verification executes 30 tests: 17 automation, 10 media, 2 native UI and 1 CLI parser test. Tests include real FFmpeg MP4 creation, a loopback model fixture, vertical 1080×1920 export, complete candidate-batch persistence before rendering, tagged ownership recovery after publication, preservation of unrelated files, subtitle refresh on retry, restart/cancellation and source retention during temporary RSS failure. Loopback fixtures are explicitly synthetic tests, not live-provider success claims.

The live RSS attempt used a public YouTube channel ID. YouTube's RSS server returned HTTP 404, including on a second public feed checked independently. The engine preserves the configured source with an explicit error and no fake videos, drafts or exports; it will retry on later checks. No local model server was available, and no live YouTube-to-AI-to-MP4 completion is claimed.

The prior f0a2207 native CI run passed Linux, Windows x64, macOS Intel and Apple Silicon, with actual native preview screenshots. The newly integrated RSS/sidebar code requires its own cross-platform CI run. No installer, signed release or bundled-runtime validation is claimed.


## Live recheck — 2026-11-03, dd2e41d

The official YouTube channel `UCBR8-60-B28hp2BmDPdntcQ` returned HTTP 200 on a fresh live request. CLI `add` saved title `YouTube`, 15 actual feed entries and exactly one queued newest entry; the remaining 14 were `discovered`. A subsequent `check` reported zero new videos. `run-once` with no configured model moved only that queued video to `needs_input`; no source media or MP4 was fabricated. This supersedes the earlier unavailable-feed observation for this check, and does not prove continuous RSS availability or successful live downloading.

The e055537 CI run (41599251849) completed successfully on all four native targets. The earlier 03ee5b8 macOS Intel failure reproduced a lock-release problem during reopening; dd2e41d adds explicit last-owner unlock and a regression covering a duplicated descriptor. Later commits still require their own CI results.


The same live source was then retried with a configured loopback model endpoint. The daemon downloaded the actual newest video `dlpFv_Irxi8` through yt-dlp and ffprobe validated the resulting local MP4. No usable platform subtitles were available, so the job ended in `needs_input` without model analysis, ASR or exports. The daemon then stopped cleanly on SIGTERM. This establishes real discovery → download → missing-caption recovery, not complete live YouTube-to-MP4 success.
