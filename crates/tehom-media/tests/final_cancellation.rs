#[cfg(unix)]
#[test]
fn cancelled_during_final_probe() {
    use std::{
        process::Command,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    if let Some(input) = std::env::var_os("TEHOM_CANCEL_TEST_INPUT") {
        let output =
            std::path::PathBuf::from(std::env::var_os("TEHOM_CANCEL_TEST_OUTPUT").unwrap());
        let marker = std::path::PathBuf::from(std::env::var_os("TEHOM_FINAL_MARKER").unwrap());
        let ack = std::path::PathBuf::from(std::env::var_os("TEHOM_CANCEL_ACK").unwrap());
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = cancelled.clone();
        let watcher = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !marker.exists() {
                assert!(Instant::now() < deadline, "final validation never started");
                std::thread::sleep(Duration::from_millis(1));
            }
            signal.store(true, Ordering::Release);
            std::fs::write(ack, b"cancelled").unwrap();
        });
        let result = tehom_media::export_cancellable(
            std::path::Path::new(&input),
            &output,
            0.,
            0.5,
            &cancelled,
        );
        watcher.join().unwrap();
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert!(!output.exists());
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.mp4");
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
                "testsrc2=size=160x90:rate=10",
                "-t",
                "1",
                "-c:v",
                "libx264"
            ])
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let probe = Command::new("sh")
        .args(["-c", "command -v ffprobe"])
        .output()
        .unwrap();
    assert!(probe.status.success());
    let wrapper = directory.path().join("ffprobe");
    std::fs::write(&wrapper, b"#!/bin/sh\nfor arg in \"$@\"; do\n case \"$arg\" in */.tehom-*.mp4)\n touch \"$TEHOM_FINAL_MARKER\"\n while [ ! -f \"$TEHOM_CANCEL_ACK\" ]; do sleep 0.01; done;;\n esac\ndone\nexec \"$TEHOM_REAL_PROBE\" \"$@\"\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![directory.path().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cancelled_during_final_probe", "--nocapture"])
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env(
            "TEHOM_REAL_PROBE",
            String::from_utf8(probe.stdout).unwrap().trim(),
        )
        .env("TEHOM_CANCEL_TEST_INPUT", &input)
        .env(
            "TEHOM_CANCEL_TEST_OUTPUT",
            directory.path().join("output.mp4"),
        )
        .env("TEHOM_FINAL_MARKER", directory.path().join("final-started"))
        .env("TEHOM_CANCEL_ACK", directory.path().join("cancel-ack"))
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}
