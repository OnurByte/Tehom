use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[test]
fn cancelling_render_never_publishes_partial_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.mp4");
    let output = directory.path().join("output.mp4");
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
                "4",
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
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let worker_output = output.clone();
    let worker = std::thread::spawn(move || {
        tehom_media::export_cancellable(&input, &worker_output, 0.0, 4.0, &worker_cancelled)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !std::fs::read_dir(directory.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".tehom-")
    }) {
        assert!(
            !worker.is_finished(),
            "render finished before cancellation could be exercised"
        );
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    cancelled.store(true, Ordering::Release);
    let error = worker.join().unwrap().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    assert!(!output.exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
