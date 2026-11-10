//! A readable source must remain inspectable when temporary-file writes fail.
#![cfg(target_os = "linux")]

use std::process::Command;

#[test]
fn real_probe_does_not_depend_on_writable_temporary_capture_files() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("actual.mp4");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=s=64x64:r=2",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-threads",
                "1",
            ])
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let original = std::fs::read(&input).unwrap();

    // Isolated test binary: failed writes return EFBIG, like the observed EDQUOT,
    // while FFprobe can still exit successfully with an empty file capture.
    unsafe {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(libc::getrlimit(libc::RLIMIT_FSIZE, &mut limit), 0);
        limit.rlim_cur = 0;
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
        assert_ne!(libc::signal(libc::SIGXFSZ, libc::SIG_IGN), libc::SIG_ERR);
    }
    let info = tehom_media::inspect(&input).unwrap();
    assert_eq!((info.width, info.height), (64, 64));
    assert!((info.duration - 1.0).abs() < 0.25);
    assert_eq!(tehom_media::export_identity(&input).unwrap(), None);
    assert_eq!(std::fs::read(&input).unwrap(), original);
}
