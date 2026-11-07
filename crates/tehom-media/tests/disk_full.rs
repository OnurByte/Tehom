//! Spec §21: "Disk dolduğunda kullanıcı dosyalarını kaybetmiyor."
//!
//! RLIMIT_FSIZE makes the encoder's writes fail part-way, the same failure
//! mode as a volume filling during render. It is process-wide, so this test
//! lives in its own binary and generates every fixture before lowering it.
#![cfg(target_os = "linux")]
use std::process::Command;

fn set_limit(resource: libc::__rlimit_resource_t, bytes: libc::rlim_t) {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: plain libc calls with a valid, initialized struct.
    unsafe {
        assert_eq!(libc::getrlimit(resource, &mut limit), 0);
        limit.rlim_cur = bytes.min(limit.rlim_max);
        assert_eq!(libc::setrlimit(resource, &limit), 0);
    }
}

#[test]
fn failed_write_during_render_keeps_user_files_and_publishes_nothing() {
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
                "3",
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
    let earlier_clip = directory.path().join("earlier-clip.mp4");
    std::fs::copy(&input, &earlier_clip).unwrap();
    let occupied = directory.path().join("occupied.mp4");
    std::fs::write(&occupied, b"user data").unwrap();
    let snapshot = |path: &std::path::Path| std::fs::read(path).unwrap();
    let (source_bytes, clip_bytes) = (snapshot(&input), snapshot(&earlier_clip));

    let output = directory.path().join("new-clip.mp4");
    // The killed encoder must not leave a core dump on the developer's machine.
    set_limit(libc::RLIMIT_CORE, 0);
    set_limit(libc::RLIMIT_FSIZE, 64 * 1024);
    let failed = tehom_media::export(&input, &output, 0.0, 3.0);
    let refused = tehom_media::export(&input, &occupied, 0.0, 1.0);
    set_limit(libc::RLIMIT_FSIZE, libc::RLIM_INFINITY);

    let failure = failed.expect_err("render must fail when writes are refused");
    assert!(
        !matches!(
            failure.kind(),
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::AlreadyExists
        ),
        "failure must come from the refused write, not validation: {failure}"
    );
    assert_eq!(
        refused.unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert!(!output.exists(), "no partial clip may be published");
    assert_eq!(snapshot(&input), source_bytes);
    assert_eq!(snapshot(&earlier_clip), clip_bytes);
    assert_eq!(snapshot(&occupied), b"user data");
    let leftovers: Vec<_> = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().starts_with(".tehom-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temporary render files left: {leftovers:?}"
    );
}
