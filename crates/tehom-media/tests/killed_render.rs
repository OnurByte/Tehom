//! Spec §21: "FFmpeg render sırasında kapatılırsa yarım dosya başarılı görünmüyor."
#![cfg(target_os = "linux")]
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

/// PIDs of ffmpeg processes whose command line mentions `folder`.
fn ffmpeg_writing_into(folder: &Path) -> Vec<String> {
    let needle = folder.to_string_lossy().into_owned();
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let pid = entry.file_name().into_string().ok()?;
            pid.parse::<u32>().ok()?;
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            (cmdline.starts_with("ffmpeg ")
                && cmdline.contains(&needle)
                && cmdline.contains(".tehom-"))
            .then_some(pid)
        })
        .collect()
}

#[test]
fn externally_killed_encoder_never_publishes_a_partial_clip() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.mp4");
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
                "testsrc2=size=1280x720:rate=30",
                "-t",
                "20",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
            ])
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let output = directory.path().join("clip.mp4");
    let (worker_input, worker_output) = (input.clone(), output.clone());
    let worker =
        std::thread::spawn(move || tehom_media::export(&worker_input, &worker_output, 0.0, 20.0));

    let deadline = Instant::now() + Duration::from_secs(20);
    let pids = loop {
        let pids = ffmpeg_writing_into(directory.path());
        let started_writing = std::fs::read_dir(directory.path()).unwrap().any(|entry| {
            let entry = entry.unwrap();
            entry.file_name().to_string_lossy().starts_with(".tehom-")
                && entry.metadata().unwrap().len() > 0
        });
        if !pids.is_empty() && started_writing {
            break pids;
        }
        assert!(
            !worker.is_finished(),
            "render finished before it could be killed"
        );
        assert!(Instant::now() < deadline, "encoder never started writing");
        std::thread::sleep(Duration::from_millis(5));
    };
    for pid in &pids {
        assert!(
            Command::new("kill")
                .args(["-KILL", pid])
                .status()
                .unwrap()
                .success()
        );
    }

    let error = worker
        .join()
        .unwrap()
        .expect_err("a killed encoder must fail the export");
    assert!(error.to_string().contains("signal: 9"), "{error}");
    assert!(!output.exists(), "partial clip was published");
    let remaining: Vec<_> = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(remaining, vec![std::ffi::OsString::from("source.mp4")]);
}
