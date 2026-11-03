# Linux native prototype evidence

Validated code: `ce7c72b0ffc12696cbb1838672576ea0b8663231` (2026-11-03).

Rust 1.99.1, GPUI Kit 0.8.1, GStreamer 1.28.8. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` and `cargo build --workspace --locked` completed successfully. The workspace test run executed eight media/process tests. No headless UI unit tests exist yet.

The native application was observed rendering decoded video on Wayland. An isolated Xvfb X11 display and independent D-Bus/GTK portal session then exercised the real controls without interacting with the daily desktop:

1. Opened a 12-second H.264/AAC, 640×360 MP4.
2. Clicked Play; the rendered frame timestamp and current-position text advanced to about 4.9 seconds.
3. Entered 0.5 and 3.5 seconds as clip bounds.
4. Opened the native Save dialog, selected a new MP4 filename and saved.
5. The UI reported the actual saved path. Independent ffprobe inspection found H.264 video, AAC audio, 640×360 geometry and exactly 3.000000 seconds duration.

`linux-x11-preview.png` is an actual captured native window, not a mockup. The X11 test used Mesa software rendering (DRI3 warning under Xvfb); it is separate from the observed Wayland desktop window.

Not yet established: timeline slider, volume control, portrait and multi-hour-source behavior, sustained memory measurements, three-platform bundled-runtime packaging, or audible sound on every OS. Playback tests decode audio and exercise media clocks; no listening test is claimed. The full product gate remains open.
