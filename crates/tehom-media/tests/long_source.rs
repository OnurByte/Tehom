#![cfg(feature = "preview")]
use std::{
    process::Command,
    time::{Duration, Instant},
};
use tehom_media::preview::Preview;

fn wait_for_frame(preview: &Preview) -> tehom_media::preview::Frame {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(frame) = preview.take_frame() {
            return frame;
        }
        assert!(
            Instant::now() < deadline,
            "no frame: {:?}",
            preview.snapshot()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn portrait_and_three_hour_sources_decode_and_seek() {
    let directory = tempfile::tempdir().unwrap();
    for (name, geometry, seconds, target, dimensions) in [
        ("portrait", "90x160", "3", 2u64, (90, 160)),
        ("three-hours", "160x90", "10800", 10798u64, (160, 90)),
    ] {
        let input = directory.path().join(format!("{name}.mp4"));
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("testsrc2=size={geometry}:rate=1"),
                    "-t",
                    seconds,
                    "-c:v",
                    "libx264",
                    "-preset",
                    "ultrafast"
                ])
                .arg(&input)
                .status()
                .unwrap()
                .success()
        );
        let preview = Preview::open(&input).unwrap();
        let frame = wait_for_frame(&preview);
        assert_eq!((frame.width, frame.height), dimensions);
        assert!(preview.snapshot().duration >= Duration::from_secs(target));
        preview.seek(Duration::from_secs(target)).unwrap();
        let frame = wait_for_frame(&preview);
        assert_eq!((frame.width, frame.height), dimensions);
        let deadline = Instant::now() + Duration::from_secs(5);
        while preview
            .snapshot()
            .position
            .abs_diff(Duration::from_secs(target))
            > Duration::from_millis(100)
        {
            assert!(
                Instant::now() < deadline,
                "long seek failed: {:?}",
                preview.snapshot()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        #[cfg(target_os = "linux")]
        if target > 10000 {
            fn resident_kib() -> u64 {
                std::fs::read_to_string("/proc/self/status")
                    .unwrap()
                    .lines()
                    .find(|line| line.starts_with("VmRSS:"))
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .parse()
                    .unwrap()
            }
            let baseline = resident_kib();
            let mut peak = baseline;
            for index in 0..30 {
                preview.seek(Duration::from_secs(index * 347)).unwrap();
                wait_for_frame(&preview);
                peak = peak.max(resident_kib());
            }
            eprintln!("three-hour source RSS baseline={baseline} KiB, peak={peak} KiB");
            assert!(
                peak.saturating_sub(baseline) < 64 * 1024,
                "decoder retained frames across seeks"
            );
        }
    }
}
