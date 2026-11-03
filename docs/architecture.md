# Tehom architecture

Rust workspace: tehom-core holds shared serialized records, tehom-media owns safe FFmpeg/ffprobe processing and optional GStreamer preview, tehom-automation owns SQLite WAL, channel/RSS discovery, durable sequential work, subtitle parsing and local-model analysis. tehom-ui uses GPUI Kit and calls these services; it constructs no production SQL, FFmpeg or downloader commands. Desktop and the tehom CLI share the same automation engine.

The engine uses exclusive data-directory ownership, transactional candidate batches, persistent export identities, validated no-clobber publication, cancellation and restart recovery. One worker bounds render/download concurrency. Desktop checks sources every 30 minutes and refreshes visible state separately; headless CLI supports periodic polling.

The user explicitly reprioritized channel automation and sidebar integration after the initial preview spike. Native preview was proven on Linux, Windows and both macOS architectures in the preceding CI run; bundled-runtime, full product and distribution gates remain outstanding. Future module splits should preserve existing tested behavior and shared types rather than duplicate the engine.
