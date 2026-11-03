use std::process::Command;
use std::sync::atomic::AtomicBool;

#[test]
fn automated_export_is_vertical_and_carries_verified_ownership() {
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
                "2",
                "-c:v",
                "libx264"
            ])
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(tehom_media::export_identity(&input).unwrap(), None);
    let output = directory.path().join("output.mp4");
    tehom_media::export_automated(
        &input,
        &output,
        0.2,
        1.2,
        &AtomicBool::new(false),
        "job-17-owned",
    )
    .unwrap();
    assert_eq!(
        tehom_media::export_identity(&output).unwrap().as_deref(),
        Some("job-17-owned")
    );
    let info = tehom_media::inspect(&output).unwrap();
    assert_eq!((info.width, info.height), (1080, 1920));
    assert!((info.duration - 1.).abs() < 0.1);
    assert!(
        tehom_media::export_automated(
            &input,
            &directory.path().join("bad.mp4"),
            0.,
            1.,
            &AtomicBool::new(false),
            "bad\nidentity"
        )
        .is_err()
    );
}
