# Native preview and export gate

User supplied the complete product specification and instructed starting from scratch. Implement inline in this workspace. The complete product scope remains in master-spec.txt; this plan covers its mandatory first gate, not a redefinition of completion.

1. Pin GPUI Kit 0.8.1, lock dependencies, compile a native window.
2. Shared media module: inspect local video, validate intervals, export H.264/AAC MP4 without overwriting existing files. Test real video and failed/partial output; publish only a completed file atomically.
3. Prototype GStreamer playbin/appsink preview on a background worker with a bounded latest-frame slot, audio, pause/resume and accurate seek. Use Kit buttons and inputs; keep subprocess and pipeline work outside UI render callbacks.
4. Wire native file picker, in/out range, export destination and explicit errors. Exercise actual preview, pause, seek, reopen and export on Linux.
5. Add platform smoke-test entry point and CI builds for Linux/macOS/Windows; record separately build, runtime and packaged-binary evidence. Do not claim remote platform validation without results.
6. Only after the preview/export gate is proven across all three platforms, proceed to the full milestones in the master specification (sources, transcript, AI, persistent jobs, editing, headless and signed distribution).

Files: apps/tehom-desktop is the prototype host; crates/tehom-media owns subprocess export and GStreamer preview; docs record evidence and decisions. No independent production engine is duplicated in UI.

Review focus: timestamp bounds and nonfinite values; existing output preservation; crash leaving no published partial MP4; frame memory and texture eviction; pause/seek synchronization; sound; errors that allow another file to be opened.
