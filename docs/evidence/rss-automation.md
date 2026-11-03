# RSS automation integration evidence

The user reprioritized the real channel/RSS workflow and requested a sidebar following the master design. The application now opens on channel entry, with native Home/Sources/Clips/Settings navigation and the explicitly supplied logo(2).svg. The supplied SVG was byte-identical to the existing original; geometry remains unchanged.

The shared tehom-automation engine persists sources, videos, candidates, export intents and settings in SQLite WAL. First channel discovery seeds the feed and queues only its newest video; later discoveries queue new entries. Automatic draft and export modes, persistent pause/resume, retry and cancellation are wired into the desktop. A standard file lock prevents two engines from recovering the same active work. A CLI uses the same engine for add/check/status/daemon commands.

Local workspace verification executes 30 tests: 17 automation, 10 media, 2 native UI and 1 CLI parser test. Tests include real FFmpeg MP4 creation, a loopback model fixture, vertical 1080×1920 export, complete candidate-batch persistence before rendering, tagged ownership recovery after publication, preservation of unrelated files, subtitle refresh on retry, restart/cancellation and source retention during temporary RSS failure. Loopback fixtures are explicitly synthetic tests, not live-provider success claims.

The live RSS attempt used a public YouTube channel ID. YouTube's RSS server returned HTTP 404, including on a second public feed checked independently. The engine preserves the configured source with an explicit error and no fake videos, drafts or exports; it will retry on later checks. No local model server was available, and no live YouTube-to-AI-to-MP4 completion is claimed.

The prior f0a2207 native CI run passed Linux, Windows x64, macOS Intel and Apple Silicon, with actual native preview screenshots. The newly integrated RSS/sidebar code requires its own cross-platform CI run. No installer, signed release or bundled-runtime validation is claimed.
