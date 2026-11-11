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

    struct RestoreLimit(libc::rlimit, libc::sighandler_t);
    impl Drop for RestoreLimit {
        fn drop(&mut self) {
            // SAFETY: restore the values saved by this isolated test process.
            unsafe {
                libc::setrlimit(libc::RLIMIT_FSIZE, &self.0);
                libc::signal(libc::SIGXFSZ, self.1);
            }
        }
    }
    // Isolated test binary: failed writes return EFBIG, like the observed EDQUOT.
    // Restore before the test runner writes its result, even during unwinding.
    let restore = unsafe {
        let mut original_limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(libc::getrlimit(libc::RLIMIT_FSIZE, &mut original_limit), 0);
        let signal = libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        assert_ne!(signal, libc::SIG_ERR);
        let restore = RestoreLimit(original_limit, signal);
        let limit = libc::rlimit {
            rlim_cur: 0,
            ..original_limit
        };
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
        restore
    };
    let info = tehom_media::inspect(&input);
    let identity = tehom_media::export_identity(&input);
    let after = std::fs::read(&input);
    drop(restore);
    let info = info.unwrap();
    assert_eq!((info.width, info.height), (64, 64));
    assert!((info.duration - 1.0).abs() < 0.25);
    assert_eq!(identity.unwrap(), None);
    assert_eq!(after.unwrap(), original);
}
