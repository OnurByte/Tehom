use std::process::Command;

#[test]
fn reads_video_geometry_and_rejects_audio_only_input() {
    let directory = tempfile::tempdir().unwrap();
    let video = directory.path().join("video.mp4");
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
            .arg(&video)
            .status()
            .unwrap()
            .success()
    );
    let info = tehom_media::inspect(&video).unwrap();
    assert_eq!((info.width, info.height), (160, 90));
    assert_eq!(info.codec, "h264");
    assert!((info.duration - 1.).abs() < 0.1);
    let audio = directory.path().join("audio.wav");
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
                "sine=frequency=440",
                "-t",
                "1"
            ])
            .arg(&audio)
            .status()
            .unwrap()
            .success()
    );
    assert!(tehom_media::inspect(&audio).is_err());
    assert!(tehom_media::export(&audio, &directory.path().join("invalid.mp4"), 0., 0.5).is_err());
    assert!(!directory.path().join("invalid.mp4").exists());
}
